# FreeSynergy Hub — let the programs find each other

**This is a building block, and it is why your calendar can know your contacts without either one owning the other.**

FreeSynergy Hub is one small background program per device. FreeSynergy programs do not look for each other, they only look for the hub. The hub collects the objects that programs choose to share, tells each program what happened in the others, and proposes matches, such as "these two contacts look like the same person".

What is different here: every program works alone without the hub and hands its changes over later. A program only sees the kinds of objects it declared it wants to read. Sensitive data is never collected, and an older copy at the hub is deleted if an object becomes sensitive. Nothing leaves your device through the hub today; it talks over a local socket only.

## What it can do today

Latest tag: `v0.4.0`.

- A daemon on a local Unix socket that refuses a second hub and cleans up after itself.
- Programs register with what they write and what they read; the hub keeps a pull position per program, which survives restarts.
- Push, pull, acknowledge and live notification of changes; limits on message sizes.
- Same-person proposals between contact cards of different programs (shared e-mail or phone).
- A client library with a "hub absent" state, so a program just keeps working when no hub runs.
- Tested end to end with the real people, tasks and calendar servers, including large transfers and restarts.
- Example: `cargo run -p fs-hub --example two_programs`.

## What is not there yet

- Linux only (Unix socket). The hub is started by hand.
- No federation, no connection between devices, no names or domains.
- The only protection is "same user on the same machine"; there are no per-program tokens.
- The hub store is not encrypted yet, which is why sensitive data stays in the program.
- Two programs editing the same object at once: the last write wins.
- The hub's own log only grows; there is no clean-up yet.
- The interface is a draft.

## Part of FreeSynergy

The hub is the connector in the middle of the programs. FreeSynergy is a family of programs that are free for everyone, forever, and that keep your data yours. The documentation, the manifesto and the overall plan live in [fs-documentation](https://github.com/FreeSynergy/fs-documentation); the reading version is at <https://freesynergy.github.io/fs-documentation/>. Start with the [manifesto](https://freesynergy.github.io/fs-documentation/manifesto/) if you want to know why we do this.


## For developers

- Workspace crates: `fs-hub-proto` (re-exports the contract), `fs-hub` (daemon and library), `fs-hub-client`, `fs-hub-e2e` (multi-program tests).
- The contract is `hub.capnp` in [fs-interfaces](https://github.com/FreeSynergy/fs-interfaces) (draft). Hub owns it. Needs Rust 1.89.
- Build and test: `cargo build --workspace` and `cargo test --workspace`.
- Design, limits and open questions: [DESIGN.md](DESIGN.md).
- Licence: MIT OR Apache-2.0.
