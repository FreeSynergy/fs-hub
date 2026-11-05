//! The hub's connecting logic, independent of the RPC layer: register, apply pushed activities,
//! hand out other programs' activities, propose matches. No domain logic (ADR 0019).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex};

use fs_hub_proto::{CONTRACT_VERSION, Manifest, ProgramInfo, Proposal, PullBatch, PushOutcome};
use fs_store::{
    Activity, ActivityFilter, ActivityId, Envelope, ObjectId, Query, Retention, Store,
    StoredObject, TursoStore,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as AsyncMutex;

use crate::{Error, Result};

const STATE_TYPE: &str = "ProgramState";
const STATE_CONTENT_TYPE: &str = "application/json";
const PULL_SCAN_BATCH: usize = 200;

/// What the hub remembers per program (cursors survive restarts).
#[derive(Clone, Debug, Serialize, Deserialize)]
struct ProgramState {
    object_id: ObjectId,
    manifest: Manifest,
    /// Last activity of the program's own log the hub applied (idempotency + ack).
    last_accepted: Option<ActivityId>,
    /// Last hub activity the program acknowledged (`ack`).
    pull_cursor: Option<ActivityId>,
    last_seen_ms: i64,
}

type Shared = Arc<AsyncMutex<ProgramState>>;

/// The hub: its own object store (everything pushed) plus a small state store for cursors.
pub struct HubCore {
    objects: Arc<TursoStore>,
    state: Arc<TursoStore>,
    programs: Mutex<HashMap<String, Shared>>,
    live: Mutex<HashMap<String, usize>>,
}

/// Decrements the live-session count of a program when the session ends.
pub struct LiveGuard {
    core: Arc<HubCore>,
    program: String,
}

impl Drop for LiveGuard {
    fn drop(&mut self) {
        let mut live = self.core.live.lock().unwrap();
        if let Some(n) = live.get_mut(&self.program) {
            *n = n.saturating_sub(1);
        }
    }
}

fn valid_program_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 64
        && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Identical apart from the store-owned timestamps.
fn same_content(a: &StoredObject, b: &StoredObject) -> bool {
    let strip = |o: &StoredObject| {
        let mut o = o.clone();
        let epoch = chrono::DateTime::<chrono::Utc>::default();
        o.envelope.updated = epoch;
        o.envelope.published = epoch;
        o.original = None;
        o
    };
    strip(a) == strip(b)
}

impl HubCore {
    /// Open (or create) the hub's databases in `dir`: `objects.db` and `state.db`.
    pub async fn open(dir: &Path) -> Result<Arc<Self>> {
        std::fs::create_dir_all(dir)?;
        let objects = Arc::new(TursoStore::open(dir.join("objects.db")).await?);
        let state = Arc::new(TursoStore::open(dir.join("state.db")).await?);
        state.set_retention(Retention::steps(20)).await?;
        let core = Arc::new(Self {
            objects,
            state,
            programs: Mutex::new(HashMap::new()),
            live: Mutex::new(HashMap::new()),
        });
        core.load_states().await?;
        Ok(core)
    }

    /// The hub's own store (read access for tests and tools; the hub is its only writer).
    pub fn store(&self) -> Arc<dyn Store> {
        self.objects.clone()
    }

    async fn load_states(&self) -> Result<()> {
        for o in self.state.query(&Query::new().of_type(STATE_TYPE)).await? {
            let st: ProgramState = serde_json::from_value(o.content)?;
            self.programs.lock().unwrap().insert(st.manifest.program.clone(), Arc::new(AsyncMutex::new(st)));
        }
        Ok(())
    }

    async fn save_state(&self, st: &ProgramState) -> Result<()> {
        let mut env = Envelope::new(STATE_TYPE, st.manifest.program.clone());
        env.id = st.object_id;
        let obj = StoredObject::new(env, STATE_CONTENT_TYPE, serde_json::to_value(st)?)
            .with_index("program", st.manifest.program.clone());
        let act = self.state.put(obj, "hub").await?;
        // The state store has no peer; keep its log small.
        self.state.mark_synced(act.id).await?;
        self.state.compact().await?;
        Ok(())
    }

    // ---- register ---------------------------------------------------------------------------

    /// A program announces itself. Returns the guard that counts the session as live.
    pub async fn register(self: &Arc<Self>, manifest: Manifest) -> Result<LiveGuard> {
        if !valid_program_name(&manifest.program) {
            return Err(Error::Invalid(format!("invalid program name {:?} (use [a-z0-9._-]+)", manifest.program)));
        }
        if manifest.contract_version != CONTRACT_VERSION {
            return Err(Error::Invalid(format!(
                "contract version {:?} not supported (hub speaks {CONTRACT_VERSION})",
                manifest.contract_version
            )));
        }
        let name = manifest.program.clone();
        let existing = self.programs.lock().unwrap().get(&name).cloned();
        match existing {
            Some(shared) => {
                let mut st = shared.lock().await;
                st.manifest = manifest;
                st.last_seen_ms = now_ms();
                self.save_state(&st).await?;
            }
            None => {
                let st = ProgramState {
                    object_id: ObjectId::new(),
                    manifest,
                    last_accepted: None,
                    pull_cursor: None,
                    last_seen_ms: now_ms(),
                };
                self.save_state(&st).await?;
                self.programs.lock().unwrap().insert(name.clone(), Arc::new(AsyncMutex::new(st)));
            }
        }
        *self.live.lock().unwrap().entry(name.clone()).or_default() += 1;
        Ok(LiveGuard { core: self.clone(), program: name })
    }

    fn shared(&self, program: &str) -> Result<Shared> {
        self.programs
            .lock()
            .unwrap()
            .get(program)
            .cloned()
            .ok_or_else(|| Error::Invalid(format!("unknown program {program}")))
    }

    /// Manifest of a registered program.
    pub async fn manifest(&self, program: &str) -> Result<Manifest> {
        Ok(self.shared(program)?.lock().await.manifest.clone())
    }

    /// All programs the hub knows (connected or not).
    pub async fn programs(&self) -> Vec<ProgramInfo> {
        let all: Vec<(String, Shared)> = self.programs.lock().unwrap().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let mut out = vec![];
        for (name, shared) in all {
            let st = shared.lock().await;
            let connected = self.live.lock().unwrap().get(&name).copied().unwrap_or(0) > 0;
            out.push(ProgramInfo {
                manifest: st.manifest.clone(),
                connected,
                last_seen_ms: st.last_seen_ms,
                last_accepted: st.last_accepted,
                pull_cursor: st.pull_cursor,
            });
        }
        out.sort_by(|a, b| a.manifest.program.cmp(&b.manifest.program));
        out
    }

    // ---- push -------------------------------------------------------------------------------

    /// Apply a program's activities. Idempotent: entries at or below the program's cursor are skipped.
    /// Sensitive objects (ADR 0029) are never collected; they are counted as `withheld`.
    pub async fn push(&self, program: &str, mut activities: Vec<Activity>) -> Result<PushOutcome> {
        let shared = self.shared(program)?;
        let mut st = shared.lock().await;
        activities.sort_by_key(|a| a.id);
        let mut out = PushOutcome::default();
        for a in activities {
            if st.last_accepted.is_some_and(|l| a.id <= l) {
                out.skipped += 1;
                continue;
            }
            match self.apply_one(&st.manifest, &a).await {
                Ok(Applied::Done) => out.accepted += 1,
                Ok(Applied::Same) => out.skipped += 1,
                Ok(Applied::Withheld) => out.withheld += 1,
                Err(e) => {
                    // One bad entry must not block the log behind it: report and move on.
                    out.problems.push(format!("{} {}: {e}", a.kind.as_str(), a.id));
                }
            }
            st.last_accepted = Some(a.id);
        }
        st.last_seen_ms = now_ms();
        out.last_accepted = st.last_accepted;
        self.save_state(&st).await?;
        Ok(out)
    }

    async fn apply_one(&self, manifest: &Manifest, a: &Activity) -> Result<Applied> {
        let program = manifest.program.as_str();
        if !a.kind.changes_state() {
            // Log-only (Invite, Accept, ...): keep it if we hold the object, else it has no anchor here.
            let Some(obj) = self.objects.get(a.object).await? else { return Ok(Applied::Same) };
            if !manifest.writes.contains(&obj.envelope.object_type) {
                return Ok(Applied::Withheld);
            }
            self.objects.log(a.kind, a.object, program, a.payload.clone()).await?;
            return Ok(Applied::Done);
        }
        let cs = a.change_set()?;
        match cs.after {
            Some(obj) => {
                if !obj.envelope.sensitive.is_empty() {
                    // Never keep a copy; and drop an older, then-unmarked copy.
                    if self.objects.get(obj.envelope.id).await?.is_some() {
                        self.objects.delete(obj.envelope.id, program).await?;
                    }
                    return Ok(Applied::Withheld);
                }
                if !manifest.writes.contains(&obj.envelope.object_type) {
                    return Ok(Applied::Withheld);
                }
                if let Some(have) = self.objects.get(obj.envelope.id).await? {
                    if same_content(&have, &obj) {
                        return Ok(Applied::Same); // e.g. a program re-pushing what it pulled
                    }
                }
                self.objects.put(obj, program).await?;
                Ok(Applied::Done)
            }
            None => {
                if self.objects.get(a.object).await?.is_some() {
                    self.objects.delete(a.object, program).await?;
                    Ok(Applied::Done)
                } else {
                    Ok(Applied::Same)
                }
            }
        }
    }

    // ---- pull -------------------------------------------------------------------------------

    /// AS2 type an activity is about (state-changing: from the images; log-only: the stored object).
    pub async fn type_of(&self, a: &Activity) -> Option<String> {
        if a.kind.changes_state() {
            let cs = a.change_set().ok()?;
            cs.after.or(cs.before).map(|o| o.envelope.object_type)
        } else {
            self.objects.get(a.object).await.ok().flatten().map(|o| o.envelope.object_type)
        }
    }

    /// Would `program` (with these `reads`) get this activity?
    pub async fn visible_to(&self, program: &str, reads: &[String], a: &Activity) -> bool {
        if a.actor == program {
            return false;
        }
        match self.type_of(a).await {
            Some(t) => reads.contains(&t),
            None => false,
        }
    }

    /// Activities of other programs after `since` (default: the hub-kept cursor), only declared `reads`.
    /// Does not move the cursor; the program calls [`HubCore::ack`] once it has applied them.
    pub async fn pull(&self, program: &str, since: Option<ActivityId>, limit: usize) -> Result<PullBatch> {
        let shared = self.shared(program)?;
        let (reads, mut cursor) = {
            let mut st = shared.lock().await;
            st.last_seen_ms = now_ms();
            (st.manifest.reads.clone(), since.or(st.pull_cursor))
        };
        let limit = limit.clamp(1, 1000);
        let mut out = PullBatch { last: cursor, ..PullBatch::default() };
        'scan: loop {
            let batch = self
                .objects
                .activities(&ActivityFilter { after: cursor, limit: Some(PULL_SCAN_BATCH), ..Default::default() })
                .await?;
            let n = batch.len();
            for a in batch {
                cursor = Some(a.id);
                if self.visible_to(program, &reads, &a).await {
                    if out.activities.len() >= limit {
                        out.more = true;
                        break 'scan;
                    }
                    out.last = Some(a.id);
                    out.activities.push(a);
                } else {
                    out.last = Some(a.id);
                }
            }
            if n < PULL_SCAN_BATCH {
                break;
            }
        }
        Ok(out)
    }

    /// The program applied everything up to `up_to`; remember it (cursors only move forward).
    pub async fn ack(&self, program: &str, up_to: ActivityId) -> Result<()> {
        let shared = self.shared(program)?;
        let mut st = shared.lock().await;
        if st.pull_cursor.is_none_or(|c| up_to > c) {
            st.pull_cursor = Some(up_to);
            self.save_state(&st).await?;
        }
        Ok(())
    }

    // ---- proposals --------------------------------------------------------------------------

    /// Same-person candidates across programs: `Profile` objects of different programs that share a
    /// normalised e-mail or phone number (the programs already normalise their index fields).
    /// Never for sensitive objects (ADR 0029, the hub holds none anyway), groups, or pairs a person
    /// already linked (`Relationship` objects). The hub only proposes; a human confirms in a program.
    pub async fn proposals(&self) -> Result<Vec<Proposal>> {
        let linked = self.linked_pairs().await?;
        let profiles = self.objects.query(&Query::new().of_type("Profile")).await?;
        let mut by_value: BTreeMap<String, Vec<(ObjectId, String, String, String)>> = BTreeMap::new();
        for o in profiles {
            if !o.envelope.sensitive.is_empty() {
                continue;
            }
            let kind = o.index.get("kind").and_then(|v| v.first()).cloned().unwrap_or_else(|| "individual".into());
            if kind == "group" {
                continue;
            }
            let origin = self.origin_of(o.envelope.id).await?;
            let mut keys = vec![];
            keys.extend(o.index.get("email").into_iter().flatten().map(|e| format!("email:{e}")));
            keys.extend(
                o.index
                    .get("phone")
                    .into_iter()
                    .flatten()
                    .filter(|p| p.trim_start_matches('+').len() >= 6)
                    .map(|p| format!("phone:{p}")),
            );
            for k in keys {
                by_value.entry(k).or_default().push((o.envelope.id, kind.clone(), origin.clone(), o.envelope.name.clone()));
            }
        }
        let mut pairs: BTreeMap<(ObjectId, ObjectId), Proposal> = BTreeMap::new();
        for (reason, items) in by_value {
            for (i, (a, ka, pa, na)) in items.iter().enumerate() {
                for (b, kb, pb, nb) in &items[i + 1..] {
                    if a == b || ka != kb || pa == pb {
                        continue;
                    }
                    let (first, second) = if a < b {
                        ((a, na, pa), (b, nb, pb))
                    } else {
                        ((b, nb, pb), (a, na, pa))
                    };
                    let key = (*first.0, *second.0);
                    if linked.contains(&key) {
                        continue;
                    }
                    let p = pairs.entry(key).or_insert_with(|| Proposal {
                        a: *first.0,
                        b: *second.0,
                        name_a: first.1.clone(),
                        name_b: second.1.clone(),
                        program_a: first.2.clone(),
                        program_b: second.2.clone(),
                        reasons: vec![],
                    });
                    p.reasons.push(reason.clone());
                }
            }
        }
        Ok(pairs.into_values().collect())
    }

    /// The program that first wrote the object into the hub.
    async fn origin_of(&self, id: ObjectId) -> Result<String> {
        let first = self
            .objects
            .activities(&ActivityFilter { object: Some(id), limit: Some(1), ..Default::default() })
            .await?;
        Ok(first.first().map(|a| a.actor.clone()).unwrap_or_default())
    }

    async fn linked_pairs(&self) -> Result<BTreeSet<(ObjectId, ObjectId)>> {
        let mut out = BTreeSet::new();
        for r in self.objects.query(&Query::new().of_type("Relationship")).await? {
            let get = |k: &str| r.index.get(k).and_then(|v| v.first()).and_then(|s| s.parse::<ObjectId>().ok());
            if let (Some(a), Some(b)) = (get("subject"), get("object")) {
                out.insert(if a < b { (a, b) } else { (b, a) });
            }
        }
        Ok(out)
    }
}

enum Applied {
    Done,
    Same,
    Withheld,
}
