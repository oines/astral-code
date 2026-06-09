# astral-code

`astral-code` is a provider-neutral coding agent harness built on the Codex
runtime architecture. The CLI command is `astral`.

This is a new project, not a Codex compatibility upgrade path. Astral uses its
own configuration and state namespace (`ASTRAL_HOME`, defaulting to
`~/.astral-code`) and does not read or migrate `~/.codex` data.

---

## Quickstart

### Installing and running Astral

Build the Rust CLI from source:

```shell
cargo build --manifest-path codex-rs/Cargo.toml -p codex-cli --bin astral
```

Then run:

```shell
./codex-rs/target/debug/astral
```

The first implementation stage keeps Codex runtime boundaries intact while
moving the user-facing project identity to Astral. Provider-neutral protocol
and Claude-ish tool flavor work will land in later stages.

## Docs

- [**Contributing**](./docs/contributing.md)
- [**Installing & building**](./docs/install.md)

This repository is licensed under the [Apache-2.0 License](LICENSE).
