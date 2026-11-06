# FreeSynergy common contract types — DRAFT 0.1 (2026-11-05)
# Shared by every program contract (ADR 0021). Evolution rules: only append, never renumber.
@0xb3f1c2d4e5a69788;

struct Uuid      { hi @0 :UInt64; lo @1 :UInt64; }          # UUIDv7 (ADR 0027)
struct Timestamp { unixMillis @0 :Int64; }
struct Cursor    { opaque @0 :Data; }                       # pagination; empty = start / end
struct Problem   {                                          # RFC 9457 shape
  type   @0 :Text;
  title  @1 :Text;
  detail @2 :Text;
  status @3 :UInt16;
}

interface Blob {                                            # bytes behind a capability (originals, attachments)
  size @0 () -> (bytes :UInt64, mediaType :Text);
  read @1 (offset :UInt64, length :UInt32) -> (data :Data);
}

# AS2 envelope (ADR 0020, 0032): every object carries it
struct Envelope {
  id           @0 :Uuid;
  iri          @1 :Text;                                    # https://<instance>/<domain>/<uuid>
  type         @2 :Text;                                    # AS2 type: Profile, Event, Note, Article, Collection …
  name         @3 :Text;                                    # derived display name
  attributedTo @4 :Text;                                    # actor IRI / DID
  to           @5 :List(Text);
  cc           @6 :List(Text);
  bto          @7 :List(Text);
  bcc          @8 :List(Text);
  audience     @9 :List(Text);
  context      @10 :Text;
  inReplyTo    @11 :Text;
  published    @12 :Timestamp;
  updated      @13 :Timestamp;
  sensitive    @14 :List(Text);                             # DPV categories (ADR 0029)
  describes    @15 :Text;                                   # AS2 Profile: whom this describes (actor IRI / DID)
  startTime    @16 :Timestamp;                              # AS2 Event
  endTime      @17 :Timestamp;                              # AS2 Event
}

# what a program offers (ADR 0021): capability ids like "message.send", "contact.create"
struct CapabilityList {
  contractVersion @0 :Text;
  ids             @1 :List(Text);
}

# live updates, shared shape for all programs
struct Change {
  union {
    created @0 :Envelope;
    updated @1 :Envelope;
    deleted @2 :Uuid;
  }
}
interface ChangeListener { change @0 (change :Change) -> stream; }
interface Subscription { }                                  # dropping it ends the subscription
