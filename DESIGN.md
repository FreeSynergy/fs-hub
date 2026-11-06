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
| `core/log_source.rs` (`LogSource`) | Repository (read) + Observer | scans the hub log from the cursor; `changes()` is the broadcast feed |
| `core/visibility.rs` (`Visibility`) | Specification | other programs' activities of declared `reads` types |
| `core/proposals/*` (`ProposalEngine`, `SharedIdentity`) | Strategy | same-person matching is replaceable; `candidates` / `pairing` / `linked` are its steps |
| `fs-hub/server/*` | Adapter | capnp-rpc `Hub`, `Session`, `Subscription` servers translate to `HubCore`; `listen` = transport, `socket` = Factory |
| `server/subscription.rs` | Observer | change feed -> program's `ChangeListener`; dropping the capability aborts the task |
| `fs-hub/daemon/*` | Facade / Template method / RAII | `Config`, `run` skeleton (open, bind, serve, clean up), `Daemon` thread handle |
| `fs-hub-client/connect.rs` | Factory | socket path -> `HubStatus` |
| `fs-hub-client/status.rs` (`HubStatus`) | State | `Connected` / `Absent`: same calls in both states, `sync_once` is a no-op when absent |
| `fs-hub-client/session.rs` (`HubSession`) | Facade | typed async methods over the command channel |
| `fs-hub-client/command.rs`, `worker/*` | Command + Adapter | each call is a `Cmd` enum value executed on the private RPC thread |
| `fs-hub-client/worker/listener.rs` | Observer | hub calls our `ChangeListener`, becomes a `ChangeNotice` channel |
| `fs-hub-client/sync/*` | Template method | push all, pull all; `apply` / `mark` are the steps |
| `fs-hub-proto/convert/*` | Adapter | one file per wire struct (ids, lists, envelope, activity, manifest, program, proposal, outcome) |
| `fs-hub-proto/{types,content}.rs` | Value objects / Specification | plain shapes; `same_content` is shared by hub and client |

## State

Version 0.1.0, local only (no remote, never pushed). Linux, Unix socket. ADR 0043 refactor done (traits first, no file over 200 lines, `#![deny(missing_docs)]`, `clippy::pedantic`); behaviour unchanged (7 e2e tests before and after, plus 2 envelope round-trip tests).

| Area | State |
|---|---|
| Contract `schema/hub.capnp` (+ copy of `common.capnp`): `Hub.register/programs/proposals`, `Session.push/pull/ack/subscribe`, `Manifest`, `Activity`, `ProgramInfo`, `Proposal` | done, draft 0.1 |
| Code generation: `capnpc-embedded` 0.2 (official compiler as WASM, no C++ toolchain) + `capnpc` 0.23 in `crates/fs-hub-proto/build.rs` | done; **generated code is committed** (`src/hub_capnp.rs`, `src/common_capnp.rs`, ADR 0040 point 4). Regenerate: `cargo build -p fs-hub-proto --features regenerate` (checked: reproduces the committed files byte for byte). The build dependencies are optional so a normal build does not compile the WASM engine |
| Daemon (`fs-hub` binary + lib): tokio + `capnp-rpc` over `$XDG_RUNTIME_DIR/freesynergy/hub.sock` (dir 0700, socket 0600), refuses a second hub, removes a stale socket file, removes the socket on shutdown | done |
| Hub store: `objects.db` (a `TursoStore` with all collected objects and the hub's own log) and `state.db` (per-program manifest + cursors) | done |
| Apply pushed activities, idempotent by activity id (per-program `last_accepted`, activity ids are strictly increasing per store); content-identical re-pushes are skipped; deletes and undo images are applied; log-only kinds (`Invite`, `Accept`, ...) are kept when the hub holds the object | done |
| Per-program pull cursor kept by the hub; `ack` moves it forward; cursors survive restart | done |
| Filtering: a program only gets activities of **other** programs whose object type it declared in `reads`; only types in `writes` are collected from it | done |
| `subscribe`: live `Change` notices (same filter) over the common `ChangeListener`; dropping the `Subscription` capability ends it | done |
| `proposals`: `Profile` objects of **different** programs sharing a normalised e-mail or phone (>= 6 digits), same `kind`, not groups, not already linked by a `Relationship` | done |
| Sensitive data (ADR 0029) | **never collected**: a push with `envelope.sensitive` is counted as `withheld`, any older copy at the hub is deleted; never proposed, never delivered |
| Client `fs-hub-client`: `connect` / `connect_at` -> `HubStatus::{Connected, Absent}`; `offer_install()` returns the message only; `HubSession::{push, pull, ack, programs, proposals, subscribe, sync_once}` | done. The RPC runs on a private thread, the session is `Send + Sync` |
| `sync_once(store)`: push unsynced -> `mark_synced`; pull -> apply into the program's store -> `ack`; pulled changes are marked synced so they do not travel back | done |
| Peer check: connection must come from the socket owner's uid (`SO_PEERCRED`) | done (minimal) |
| Tests | 7 end-to-end (`crates/fs-hub/tests/{sharing,proposals,push_restart,cursors,subscribe,absent,refusal}.rs`, shared `common`) + 2 envelope round-trip tests (`fs-hub-proto/tests/envelope.rs`); the e2e tests use real `fs-contacts-core` + `fs-calendar-core`, own stores): sharing contact -> event, updates/deletes, no echo, declared-types filter, proposals without sensitive data, idempotent push and restart, hub-kept pull cursor, subscribe, no hub / stale socket / hub appears later, second hub refused, bad manifest |
| Example | `cargo run -p fs-hub --example two_programs` |

## Crates

* `fs-hub-proto`: schema, generated code, conversions wire <-> `fs-store` types. A third crate beyond the two asked for, so daemon and client share the contract without depending on each other.
* `fs-hub`: `HubCore` (connecting logic, no RPC, behind the traits `ProgramRegistry`, `ActivitySink`, `ActivitySource`, `ProposalEngine`, `CursorRepository`), `server` (RPC adapter), `Daemon` (own thread, for tests/embedding), binary.
* `fs-hub-client`: what programs embed.
* Dependencies: `capnp`/`capnp-rpc`/`capnpc` 0.23 (MIT), `capnpc-embedded` 0.2 (MIT, build only, optional), `tokio`, `tokio-util`, `futures`, `serde`, `tracing`, `fs-store` (path). `capnp-rpc` 0.27 exists; 0.23 was taken to match `capnp`/`capnpc` 0.23.

## Decisions

* **Hub log = delivery log.** The hub applies a pushed object change with `store.put/delete`; its own activity log (UUIDv7) is what `pull` reads. The hub activity's `actor` is the **program name** (the original actor string stays only in the program's log). A program therefore never receives its own changes. On the receiving side the actor is written as `hub:<program>`.
* **Wire format.** `Activity` carries the typed `Envelope` (routing summary) and the full before/after images as JSON `payload` (domain content is JSON in the standards anyway). `common.capnp`'s `Envelope` now carries `describes`, `startTime`, `endTime` (fields @15-@17, schema copy updated, code regenerated); `write_envelope`/`read_envelope` map them. The JSON payload still carries the complete images and stays authoritative for content.
* **Cursors.** Push: the program's own log is the cursor (`last_accepted`); a bad single entry is reported in `problems` and the cursor moves past it (one bad entry must not block the log). Pull: `since` zero = hub-kept cursor; `pull` does not move it, `ack` does, so a crash between apply and ack only repeats work (applying is idempotent: identical content is skipped).
* **No echo.** The client marks the log entries it wrote while applying pulled changes as synced (only when no foreign unsynced entry precedes them, because `mark_synced` acknowledges a prefix). If any slip through, the hub skips them as content-identical.
* **One bad program cannot see more than it declared**: `reads` filter on the hub side. `writes` is enforced too (undeclared types are `withheld`).
* **Proposals only across programs.** Duplicates inside one program are that program's own business (`fs-contacts-core` finds them). The hub only proposes; a human confirms in a program, which writes the `Relationship` (`owl:sameAs`) that the hub then respects.
* **Hub store is unencrypted** (no key vault yet), which is why sensitive objects are refused instead of stored.

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
* Limits: batch sizes are fixed (push 100, pull 200 default); no back-pressure on `subscribe`; `Activity.at` travels with millisecond precision.

## Needed elsewhere

* **`common.capnp` `Envelope`**: done (`describes`, `startTime`, `endTime`). Open: `iri` nullable semantics ("" = none is used now).
* **fs-store**: (a) `Store::put` always stamps `updated`/creates a new activity; a "apply remote change without a new local log entry as unsynced" (or `put_synced`) would remove the need for the client's mark-synced dance and the prefix race; (b) `ActivityFilter` has no object-type filter, so the hub reads and parses payloads to filter by type; (c) `mark_synced` is prefix-only; (d) an `ObjectId` can only be UUIDv7, so the hub cannot derive deterministic ids for its state objects.
* **fs-contacts**: index `kind`, `email`, `phone` are read by the hub's proposals; they should be documented as part of the hub contract (the hub knows these index keys). Better: a generic "identity keys" index convention in `data/<domain>/standards.md`.
* **fs-calendar**: `Event` participants are only e-mail strings; a hub-level "participant -> contact" resolution would need a convention for which index key carries identity (`participant_email` vs contacts' `email`).
* **ADR 0019** names the socket but not the contract version negotiation: 0.1 requires an exact match for now.

## Open questions

* Should the hub keep objects whose type no other program reads (it does, as the central copy)?
* Should proposals for a single program's own duplicates also be shown, or stay with the program?
* Who stores "dismissed" proposals: the hub (shared) or the program (as contacts' open point says)?
* Is the hub log's actor = program name acceptable, or should the original actor (person) be kept in an extra field?
