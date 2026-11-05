# FreeSynergy hub contract — DRAFT 0.1 (ADR 0019, 0021, 0029).
# Evolution rules: only append fields/methods, never renumber, never reuse an ordinal.
@0xd2a7c5e91f0b43a8;

using Common = import "common.capnp";

# What a program tells the hub when it registers (ADR 0019: "which data types it reads/writes").
struct Manifest {
  program         @0 :Text;                    # stable name: [a-z0-9._-]+
  version         @1 :Text;                    # program version
  contractVersion @2 :Text;                    # hub contract version the program was built for
  reads           @3 :List(Text);              # AS2 types it wants to receive (Profile, Event, ...)
  writes          @4 :List(Text);              # AS2 types it pushes
  contentTypes    @5 :List(Text);              # content standards, e.g. application/jscontact+json
  capabilities    @6 :Common.CapabilityList;   # capability ids the program offers (ADR 0021)
}

# One entry of a program's activity log (ADR 0020). Object content travels as JSON in
# `payload` (the domain standards are JSON); the typed `envelope` is the routing summary.
struct Activity {
  id        @0 :Common.Uuid;                   # UUIDv7; id order is log order
  kind      @1 :Text;                          # AS2 verb: Create, Update, Delete, Invite, ...
  object    @2 :Common.Uuid;
  actor     @3 :Text;
  at        @4 :Common.Timestamp;
  hasTarget @5 :Bool;                          # Undo: the reverted activity
  target    @6 :Common.Uuid;
  envelope  @7 :Common.Envelope;               # summary of the object (after image, else before image)
  payload   @8 :Data;                          # JSON: before/after images or the caller payload
}

struct ProgramInfo {
  manifest     @0 :Manifest;
  connected    @1 :Bool;                       # at least one live session
  lastSeen     @2 :Common.Timestamp;
  lastAccepted @3 :Common.Uuid;                # last program activity the hub applied
  pullCursor   @4 :Common.Uuid;                # last hub activity the program acknowledged
}

# A candidate "same person" pair (ADR 0019 point 5). The hub only proposes; a human confirms
# in a program. Never built from sensitive objects (ADR 0029).
struct Proposal {
  a        @0 :Common.Uuid;
  b        @1 :Common.Uuid;
  nameA    @2 :Text;
  nameB    @3 :Text;
  programA @4 :Text;
  programB @5 :Text;
  reasons  @6 :List(Text);                     # e.g. "email:ada@example.org"
}

interface Session {
  # program -> hub. Idempotent: entries at or below the hub's cursor are skipped.
  push @0 (activities :List(Activity)) -> (
    lastAccepted :Common.Uuid,                 # acknowledge: the program marks its log synced up to here
    accepted     :UInt32,
    skipped      :UInt32,                      # duplicates / already known
    withheld     :UInt32,                      # sensitive (ADR 0029) or not a declared `writes` type
    problems     :List(Common.Problem));

  # hub -> program: activities of OTHER programs, only declared `reads` types. `since` zero =
  # continue from the hub-kept cursor of this program. Does not advance the cursor; call `ack`.
  pull @1 (since :Common.Uuid, limit :UInt32) -> (
    activities :List(Activity),
    last       :Common.Uuid,                   # scanned up to here (ack this)
    more       :Bool);

  ack @2 (upTo :Common.Uuid) -> ();

  # live notifications about other programs' changes (same filter as pull)
  subscribe @3 (listener :Common.ChangeListener) -> (subscription :Common.Subscription);
}

interface Hub {
  register  @0 (manifest :Manifest) -> (session :Session, hubVersion :Text);
  programs  @1 () -> (programs :List(ProgramInfo));
  proposals @2 () -> (proposals :List(Proposal));
}
