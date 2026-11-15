# Penelope

Deterministic state machines and reliable execution for Rust. Start with a typed
state transition; add saga coordination, recoverable tasks, SQLite persistence
or timeline correction as needed.

Transitions prepare a decision before publishing it. Durable work is recorded
before dispatch, and uncertain external effects require reconciliation.

## Get started

This branch contains the upcoming 0.5 API. Use Rust 1.89 or newer; repository
builds use the toolchain pinned in `rust-toolchain.toml`.

```sh
cargo new my-app
cd my-app
cargo add penelope --git https://github.com/STEXS-Technologies/penelope --branch refactor/unified-state-execution
```

Follow the [first machine tutorial](docs/getting-started.md) for a complete
`src/main.rs` you can run with `cargo run`.

To try the included example first:

```sh
git clone --branch refactor/unified-state-execution https://github.com/STEXS-Technologies/penelope.git
cd penelope
cargo run -p penelope --example counter_machine --locked
```

For the published API, use `cargo add penelope@0.4.0` and its
[versioned API reference](https://docs.rs/penelope/0.4.0/penelope/).
The machine tutorial uses the unreleased branch.

## Choose what you need

| Need | Start here |
| --- | --- |
| Typed state transitions | [First machine](docs/getting-started.md) · `penelope::machine` |
| Multi-step work with retries and compensation | [Trade example](crates/penelope/examples/trade.rs) · `penelope::saga` |
| Checkpoints and recovery after interruption | [Recoverable task example](crates/penelope-executor/examples/recoverable_task.rs) · `penelope::task` |
| Durable local storage | [SQLite adapter](crates/penelope-sqlite/README.md) · [machine/task/saga progression](docs/getting-started.md) |
| Correct and replay earlier inputs | [Timeline quickstart](docs/timeline-examples.md#quickstart) · optional `timeline` feature |

Applications supply workers and choose their storage and runtime adapters.
The portable task core and timeline crate support `no_std`; the facade is hosted.

[Documentation](docs/README.md) · [0.5 progress](docs/0.5/README.md) ·
[Changelog](CHANGELOG.md) · [Security](SECURITY.md)

Licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE).
