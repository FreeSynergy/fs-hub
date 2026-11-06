# fs-hub – Design

`fs-hub` is the FreeSynergy hub (ADR 0019): **one small daemon per device**. Programs never look for each other, only for the hub.
The hub collects the objects of all programs in its own `fs-store`, keeps a cursor per program, and proposes matches.
It contains only connecting logic, no domain logic. Programs work alone without it and hand over their log later.

## Structure and pattern map (ADR 0043)

Every file is one concept, <= 200 lines (tests included; e2e tests split by feature in `crates/fs-hub/tests/`, helpers in
`tests/common`), `lib.rs`/`mod.rs` only declare and re-export, every module doc has a `Pattern:` line. Programs depend
on traits; concrete types are wired in `core/hub.rs`.

| Module | Pattern | Why |
|---|---|---|
| `fs-hub/core/hub.rs` (`HubCore`) | Facade | one entry for RPC layer and tests; `open_with` swaps the proposal strategy |
| `core/registry.rs` (`ProgramRegistry`, `StoredRegistry`) | Strategy + Registry | register / manifest / programs / exclusive `ProgramLock` per program |
| `core/cursors.rs` (`CursorRepository`, `StoreCursors`), `core/state.rs` | Repository + Value object | per-program cursors persisted in `state.db`, interface independent of the store |
| `core/presence.rs` (`Presence`, `LiveGuard`) | RAII / Guard | live-session count, uncounted on drop |
| `core/activity.rs` (`ActivitySink`, `ActivitySource`) | Strategy (interface segregation) | push side and pull side are separate interfaces |
| `core/collector.rs`, `core/apply.rs` (`ObjectCollector`) | Command executor | each pushed activity is applied to the hub store; sensitive / undeclared types withheld |
| `core/log_source.rs` (`LogSource`) | Repository (read) + Observer | scans the hub log from the cursor, cuts by count and size; `changes()` is the broadcast feed |
| `core/originals.rs` (`OriginalTimes`, `StoreOriginals`) | Repository | when the program did what a hub log entry records, kept beside the log |
| `core/marked.rs` (`MarkedRemovals`, `StoreMarked`) | Repository | objects removed because they were marked sensitive; keeps their old entries from going out |
| `core/visibility.rs` (`Visibility`) | Specification | other programs' activities of declared `reads` types |
| `core/proposals/*` (`ProposalEngine`, `SharedIdentity`) | Strategy | same-person matching is replaceable; `candidates` / `pairing` / `linked` are its steps |
| `fs-hub/server/*` | Adapter | capnp-rpc `Hub`, `Session`, `Subscription` servers translate to `HubCore`; `listen` = transport, `socket` = Factory |
| `server/subscription.rs` | Observer | change feed -> `rpc::forward` -> program's `ChangeListener`; every end the program did not ask for goes through `ended`; dropping the capability stops it |
| `fs-hub/daemon/*` | Facade / Template method / RAII | `Config`, `run` skeleton (open, bind, serve, clean up), `Daemon` thread handle |
| `fs-hub-client/connect.rs` | Factory | socket path -> `HubStatus` |
| `fs-hub-client/status.rs` (`HubStatus`) | State | `Connected` / `Absent`: same calls in both states, `sync_once` is a no-op when absent |
| `fs-hub-client/session.rs` (`HubSession`) | Facade | typed async methods over the command channel |
| `fs-hub-client/push_plan.rs` | Iterator-style batching | a push in log order, in pieces that fit a message; sensitive images become stubs; what cannot go stays as a problem |
| `fs-hub-client/envelopes.rs` (`EnvelopeCursor`) | Newtype | a reader of summaries has its own place, which `ack` does not take |
| `fs-hub-client/command.rs`, `worker/*` | Command + Adapter | each call is a `Cmd` enum value executed on the private RPC thread |
| `fs-hub-client/worker/listener.rs` | Observer | hub calls our `ChangeListener`, `change` and `ended` become a `ChangeNotice` channel |
| `fs-hub-client/sync/*` | Template method | push all, pull all; `apply` / `mark` are the steps |
| `fs-hub-proto/convert/*` | Adapter | one file per wire struct (ids, lists, activity, manifest, program, proposal, outcome); envelope and uuids come from `fs_interfaces::rpc` |
| `fs-hub-proto/limits.rs` (`PageBudget`, `ScanSize`, `reader_options`) | Specification + Value object | what fits a message, in one place for hub and client |
| `fs-hub-proto/delivery.rs` (`blank_for_delivery`) | Specification | the one place that names and blanks what a reader does not get |
| `fs-hub-proto/{types,content}.rs` | Value objects / Specification | plain shapes; `same_content` is shared by hub and client |

## State

Version 0.3.0 (contract and `rpc` from `fs-interfaces` v0.24.0; capnp/capnp-rpc 0.27; Rust 1.89, which `rpc` needs; no tag yet). Linux, Unix socket. ADR 0043 refactor done (traits first, no file over 200 lines, `#![deny(missing_docs)]`, `clippy::pedantic`); behaviour unchanged (7 e2e tests before and after, plus 2 envelope round-trip tests). Test dependency bumped to fs-calendar v0.4.0 (calendar provider rule); 9 tests before and after.

| Area | State |
|---|---|
| Contract `hub.capnp` (+ `common.capnp`), both in `fs-interfaces`; this program owns `hub.capnp`: `Hub.register/programs/proposals`, `Session.push/pull/ack/subscribe`, `Manifest`, `Activity`, `ProgramInfo`, `Proposal` | done, draft 0.1 |
| Code generation: in `fs-interfaces` (our `capnp-compiler` + `capnpc` 0.27, output identical to `capnpc-embedded`). `fs-hub-proto` re-exports `hub_capnp` / `common_capnp`; the local `schema/`, `build.rs` and `regenerate` feature are gone | done |
| Daemon (`fs-hub` binary + lib): tokio + `capnp-rpc` over `$XDG_RUNTIME_DIR/freesynergy/hub.sock` (dir 0700, socket 0600), refuses a second hub, removes a stale socket file, removes the socket on shutdown | done |
| Hub store: `objects.db` (a `TursoStore` with all collected objects and the hub's own log) and `state.db` (per-program manifest + cursors) | done |
| Apply pushed activities, idempotent by activity id (per-program `last_accepted`, activity ids are strictly increasing per store); content-identical re-pushes are skipped; deletes and undo images are applied; log-only kinds (`Invite`, `Accept`, ...) are kept when the hub holds the object | done |
| Per-program pull cursor kept by the hub; `ack` moves it forward; cursors survive restart | done |
| Size limits: pull answers and push batches cut by size, one object at most 16 MiB, reader limit 128 MiB on both sides (see Limits) | done |
| `pull(..., envelopeOnly)`; `HubSession::pull_envelopes` with its own `EnvelopeCursor`; `PullBatch::notes` | done |
| `Activity.originalAt` sent, kept, delivered; `originalActor` empty for everybody | done |
| Filtering: a program only gets activities of **other** programs whose object type it declared in `reads`; only types in `writes` are collected from it | done |
| `subscribe`: live `Change` notices (same filter) over the common `ChangeListener`; dropping the `Subscription` capability ends it; `ended` on lag, overflow or an untellable change, `ChangeNotice::Ended` in the client; log-only activities are not told (they change no object) | done |
| `proposals`: `Profile` objects of **different** programs sharing a normalised e-mail or phone (>= 6 digits), same `kind`, not groups, not already linked by a `Relationship` | done |
| Sensitive data (ADR 0029) | **never collected**: a push with `envelope.sensitive` is counted as `withheld`, any older copy at the hub is deleted; never proposed, never delivered |
| Client `fs-hub-client`: `connect` / `connect_at` -> `HubStatus::{Connected, Absent}`; `offer_install()` returns the message only; `HubSession::{push, pull, ack, programs, proposals, subscribe, sync_once}` | done. The RPC runs on a private thread, the session is `Send + Sync` |
| `sync_once(store)`: push unsynced -> `mark_synced`; pull -> apply into the program's store -> `ack`; push again what is left; `hub:*` entries never travel back | done |
| Peer check: connection must come from the socket owner's uid (`SO_PEERCRED`) | done (minimal) |
| Tests | end-to-end in `crates/fs-hub/tests/`: `pull_size` (80 x 1 MiB, in order across pages, `sync_once` both ways), `oversize`, `envelope_only`, `original_time`, `lag`, `ended`, `old_entries`, `blanked_fields` (the full round trip), `sensitive_marks`, `no_rollback`, `transient` (real stores with their fault hook), `memory` (peak of the real binary) on a real hub over a real socket (`common/raw` speaks the wire without the client), plus `{sharing,proposals,push_restart,cursors,subscribe,absent,refusal}.rs`; `fs-hub-proto/tests/{envelope,pushed}.rs`, unit tests in `limits.rs`, `push_plan.rs`, `worker/listener.rs`; the older e2e tests use real `fs-people-core` + `fs-calendar-core`, own stores): sharing contact -> event, updates/deletes, no echo, declared-types filter, proposals without sensitive data, idempotent push and restart, hub-kept pull cursor, subscribe, no hub / stale socket / hub appears later, second hub refused, bad manifest |
| Example | `cargo run -p fs-hub --example two_programs` |

## Crates

* `fs-hub-proto`: re-export of the contract from `fs-interfaces`, conversions wire <-> `fs-store` types. A third crate beyond the two asked for, so daemon and client share the contract without depending on each other.
* `fs-hub`: `HubCore` (connecting logic, no RPC, behind the traits `ProgramRegistry`, `ActivitySink`, `ActivitySource`, `ProposalEngine`, `CursorRepository`), `server` (RPC adapter), `Daemon` (own thread, for tests/embedding), binary.
* `fs-hub-client`: what programs embed.
* Dependencies: `capnp`/`capnp-rpc` 0.27 (MIT), `fs-interfaces` (git tag v0.24.0, feature `rpc`), `tokio`, `tokio-util`, `futures`, `serde`, `tracing`, `fs-store` (path).

## Decisions

* **Hub log = delivery log.** The hub applies a pushed object change with `store.put/delete`; its own activity log (UUIDv7) is what `pull` reads. The hub activity's `actor` is the **program name** (the original actor string stays only in the program's log). A program therefore never receives its own changes. On the receiving side the actor is written as `hub:<program>`.
* **Wire format.** `Activity` carries the typed `Envelope` (routing summary) and the full before/after images as JSON `payload` (domain content is JSON in the standards anyway). `common.capnp`'s `Envelope` now carries `describes`, `startTime`, `endTime` (fields @15-@17, schema copy updated, code regenerated); `write_envelope`/`read_envelope` map them. The JSON payload still carries the complete images and stays authoritative for content.
* **Cursors.** Push: the program's own log is the cursor (`last_accepted`); a bad single entry is reported in `problems` and the cursor moves past it (one bad entry must not block the log). Pull: `since` zero = hub-kept cursor; `pull` does not move it, `ack` does, so a crash between apply and ack only repeats work (applying is idempotent: identical content is skipped).
* **No echo.** Entries the client wrote while applying pulled changes have the actor `hub:<program>` and are never pushed: `push_all` passes them and marks them synced. Pushing one could roll the hub back, since it may hold a newer version by then; that can happen after a crash and behind a foreign unsynced entry, because `mark_synced` acknowledges a prefix. Identical content is still skipped at the hub. `HubSession::push` is not filtered: it sends what it is given.
* **A failure that may pass moves no cursor.** A bad entry (not a change set, refused by a store rule) is reported and passed. A backend or key error is nothing the entry did: the hub keeps what it applied, stops with an error and the cursor in front of the entry; `sync_once` acknowledges what it applied and stops. Problems reported earlier in such a failed push are lost with it. Only a lost `originalAt` does not stop anything.
* **One bad program cannot see more than it declared**: `reads` filter on the hub side. `writes` is enforced too (undeclared types are `withheld`).
* **Proposals only across programs.** Duplicates inside one program are that program's own business (`fs-people-core` finds them). The hub only proposes; a human confirms in a program, which writes the `Relationship` (`owl:sameAs`) that the hub then respects.
* **Hub store is unencrypted** (no key vault yet), which is why sensitive objects are refused instead of stored.
* **A sensitive object says only that it is.** Each image is looked at on its own: a sensitive one travels as a stub with its id, a generic marker, a neutral name and type, zero times; the entry's `at` is zero and `actor` blank on every push. The other image of the same change stays, or taking a mark off would destroy the object. The hub needs no more to withhold it and drop an older copy. A log-only entry about a sensitive object, or one the store no longer has, does not leave the program and counts as withheld. Limit: ids are UUIDv7, so the id itself holds its creation time.
* **Marked later.** When a mark reaches the hub, the older copy is deleted and the object goes on a persistent list (`core/marked.rs`), written first, so a crash cannot let anything out. Its entries up to and including that delete go out without payload, without time and with a stub as envelope; the times are forgotten in `state.db` (the last writes there stay in its log until it is next compacted). Readers carry out the delete, and pass the rest without a failure. Newer versions, if the mark comes off, are delivered as usual. **Limit, plainly: the old content is still on the hub's disk**, in the log of `objects.db`, including the delete's `before` image. It is only not delivered. The purge of a marked object's old log entries waits for the log purge API `fs-store` is getting.
* **Blanked at delivery (PROVISIONAL; the strategy session has the question).** The cores set `attributedTo` and `to` to the person's IRI (`fs-people`, `fs-wiki`, `fs-calendar`), which would tell every reader what `originalActor` keeps back. So `attributedTo`, `to`, `cc`, `bto`, `bcc`, `audience` are blank in everything that leaves the hub: the typed envelope, the envelopes inside the images, live notices. `actor` is blank on push. Content such as a JSContact card stays: it is the object itself, as do `iri`, `context`, `describes` and caller payloads of log-only entries. One place: `blank_for_delivery` in `fs-hub-proto/delivery.rs`, because a later release per person and per program will control these same fields. Nothing is lost: a blank field in a pushed object does not overwrite the hub's value, nor one in a pulled object the local one (`keep_held_back`), and `same_content` ignores them. The price: a field its owner really emptied keeps the old value at the hub, unseen, until the wire can tell blanked from emptied.
* **`originalAt`: the time the program did it.** Sent with the push, kept in `state.db` by the id of the hub's log entry (the store sets the log's own `at`), delivered with `pull`, so `at` stays the time the hub applied it. Never for a withheld or sensitive object (ADR 0029): the client does not send it, the hub keeps none for such an entry, also not for the delete that removes an older copy, and does not deliver one. Written after the change, not in one transaction: a crash in between loses only the time.
* **`originalActor` stays empty and the hub delivers it to nobody** (strategy decision, 2026-11-06). Who did something can only be released by the person; a manifest cannot grant this, since a program would be granting it for itself. The hub neither reads nor stores the field, whatever a program sends. `actor` remains the program's name.
* **`Lagged` ends the feed.** A lagging program could catch up with `pull`, but the contract only has `ended` to tell it that changes were missed, and a feed must never end silently. So it ends; the program subscribes again and then pulls, in that order.

## Limits

A message above the reader's limit ends the whole connection with all its capabilities, so nobody may send one. All in `fs-hub-proto/limits.rs`:

| Limit | Value | Why |
|---|---|---|
| `rpc::MAX_PAGE_BYTES` | 32 MiB | a pull answer and a push piece end before the next activity would pass it; at least one activity, so paging advances |
| `MAX_OBJECT_BYTES` (`rpc::MAX_INTAKE_BYTES`) | 16 MiB of JSON per image | the same limit every server has |
| Largest allowed activity | 48 MiB | two images and the envelope summary |
| `MAX_HUB_MESSAGE_BYTES` (2 x `rpc::MAX_MESSAGE_BYTES`) | 128 MiB, `ReaderOptions` on both sides | holds the largest allowed activity with room to spare, and any entry an older hub could have logged up to about this size |

* **Weight, not count.** An activity weighs its JSON payload, so an update counts its object twice, plus `rpc::envelope_weight` and a fixed frame. The count limit (`limit`, default 200, at most 1000) still applies. The cursor logic is unchanged: an answer ends before the activity that does not fit, `last` is the last one scanned before it, `more` says one is waiting.
* **Above the limit at push.** The hub checks the `after` image (log-only: the payload) after the sensitive and `writes` rules, reports a problem of type `too-large` (413), and the cursor moves past it, as for any bad entry. The hub keeps its older version of that object until a smaller one comes; the pushing program is the one told, readers are not. `HubSession::push` checks too, because an object above the message limit could not reach the hub at all: the entry stays home, the outcome has the same problem and `last_accepted` moves past it. There an undeclared large type counts as a problem, not as `withheld`, since the client does not know the rule. A large `before` image is left out: the hub keeps its own.
* **Above the message limit in the hub's log.** Only an entry older than these limits can be. It goes without `before`, which a reader does not need. If `after` alone is too large, it goes with an empty payload, so its reader moves past it: `sync_once` reports it in `failed`, `envelopeOnly` still shows what it is. A delete is carried out with or without a payload.
* **Memory.** The store cannot filter by size, so hub and client read one entry, then 8 at a time, and one at a time for the rest of a call once an entry of 256 KiB was seen (`ScanSize`). Envelopes are read from the JSON without copying the content, payloads are taken, not cloned; no store change was needed. Measured on the real binary, a page of 28 MiB (entries of 14 MiB): peak 169 MiB, 981 MiB with the former 32 entries at once.
* **Summaries do not move the cursor.** The hub's cursor means "the content up to here is applied". `pull_envelopes` gives its own `EnvelopeCursor`, a type `ack` does not take (checked by a `compile_fail` doc test), and the hub does not remember it. On the wire `ack` is one method: a program without `fs-hub-client` can still get this wrong.

## Not done yet

* **Socket activation** (systemd `.socket` unit / starting the hub on demand). Today the hub is started by hand.
* **Installing the hub on demand**: `offer_install()` only returns text.
* **mDNS / DNS-SD, names, domains, ACME, WebFinger** (ADR 0028).
* **OpenID Federation, P2P, instance-to-instance sync** (ADR 0035, 0036).
* **Authentication of the socket peer** beyond "same uid": no program identity proof (any process of the user can claim any program name), no encryption of the socket (local Unix socket, so none on the wire), no per-program tokens.
* **Encrypted hub store** and the key vault (ADR 0024); explicit per-recipient release of sensitive data (ADR 0029 point 2). Until then sensitive data stays in the program.
* Windows named pipes; JSON/HTTPS, CLI and MCP faces generated from the schema (ADR 0021).
* Hub-side log compaction/retention (the hub log only grows; `state.db` is compacted); tombstones for deleted objects of programs that were offline long.
* Conflict handling: concurrent edits of one object in two programs are last-write-wins at the hub (ADR 0020 "Konfliktzustand" is open).
* Addressing (`to`/`cc`) is carried but not yet used for filtering.
* The calendar-participant -> contact link is not made by the hub; the calendar finds the pulled contact by e-mail itself.
* Limits: batch sizes are fixed (pull 200 default), all limits above are constants; `Activity.at` travels with millisecond precision. Before the first large entry is seen, a scan can hold 8 entries of any size.
* `proposals` is not cut by size: names up to an object's size, in numbers, could pass a message. Nothing near that has been seen.
* **`rpc` transport not adopted yet**: in v0.24.0 `rpc::serve` and `rpc::connect` always use default `ReaderOptions`, and `BoundSocket`'s listener is private, so the hub cannot use them with its raised limit. The next `fs-interfaces` tag brings `serve_with`, `connect_with` and `Problem::read`, which let the hub drop its transport and `read_problem`. Until then it keeps its own accept loop, `bind_socket` and peer check, without the lock file and the euid check of `rpc`. The client does not check who serves the socket. Taken from `rpc`: envelope, uuids, default socket path, feed forwarding, `Problem`, limits.
* A consumer whose `change` never returns only stops the feed for itself; `ended` cannot overtake that call (see Needed elsewhere), and if the feed lagged meanwhile nothing more arrives to trigger the overflow rule. It ends with the connection.

## Needed elsewhere

* **`common.capnp` `Envelope`**: done (`describes`, `startTime`, `endTime`). Open: `iri` nullable semantics ("" = none is used now).
* **fs-store**: (a) `Store::put` always stamps `updated`/creates a new activity; a "apply remote change without a new local log entry as unsynced" (or `put_synced`) would remove the need for the `hub:` actor convention; a log purge is coming, and the old entries of a marked object wait for it; a size column in `ActivityFilter` would let a scan stop before reading a large entry; (b) `ActivityFilter` has no object-type filter, so the hub reads and parses payloads to filter by type; (c) `mark_synced` is prefix-only; (d) an `ObjectId` can only be UUIDv7, so the hub cannot derive deterministic ids for its state objects.
* **fs-contacts**: index `kind`, `email`, `phone` are read by the hub's proposals; they should be documented as part of the hub contract (the hub knows these index keys). Better: a generic "identity keys" index convention in `data/<domain>/standards.md`.
* **fs-calendar**: `Event` participants are only e-mail strings; a hub-level "participant -> contact" resolution would need a convention for which index key carries identity (`participant_email` vs contacts' `email`).
* **`fs-interfaces`**: (a) `rpc::serve`/`connect` need a way to pass `ReaderOptions`, or `BoundSocket` an accessor for its listener; (b) `common.capnp` promises `ended` when "its listener failed", but capnp-rpc 0.27 fails every later call on a capability after a stream call answered with an error, and later calls wait while a `change` is under way, so a listener that failed or never answers cannot be told (`local.rs`); the hub ends the feed and releases the listener, which is all that consumer sees. Our client never answers `change` with an error for that reason; (c) `rpc::Problem` has `write` but no reader, the hub has its own `read_problem`; (d) `rpc::read_envelope` refuses an envelope without timestamps, which `Activity.envelope` of a log-only entry was until now: pull answers now carry the stored object's envelope.
* **`fs-history-hub`**: pins hub v0.2.0. At the next bump `DEFAULT_MAX_BATCH` and the "too large" advice can go, `pull_envelopes` fits it (it reads only summaries), and `tests/large_batch.rs` expects the old failure.
* **ADR 0019** names the socket but not the contract version negotiation: 0.1 requires an exact match for now.

## Open questions

* Should the hub keep objects whose type no other program reads (it does, as the central copy)?
* Should proposals for a single program's own duplicates also be shown, or stay with the program?
* Who stores "dismissed" proposals: the hub (shared) or the program (as contacts' open point says)?
