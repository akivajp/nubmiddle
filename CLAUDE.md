# nubmiddle — notes for coding agents

## Language conventions (override the user's global defaults)

This project targets international users, so:

- **Code comments, doc comments, CLI messages (`--help`, logs, errors) and commit messages are in English.**
- **Documentation is English first, with a Japanese counterpart side by side**
  (`README.md` + `README.ja.md`; future docs follow the same `*.md` / `*.ja.md` pairing).
  Keep both versions in sync when editing either one.
- Conversation with the user is still in Japanese.

## Build and test

- Cross-compile from WSL: `cargo win` / `cargo win --release`
  (alias in `.cargo/config.toml`; needs `mingw-w64` and the `x86_64-pc-windows-gnu` target).
- `cargo test` runs on Linux: `gesture` and `config` are OS-independent on purpose.
  Keep Windows API calls out of them so the core logic stays testable.
- Run `cargo fmt` and `cargo clippy --target x86_64-pc-windows-gnu` before committing.

## Design notes

- Keep the hook callback (`hook.rs`) minimal: Windows silently removes low-level hooks that exceed
  `LowLevelHooksTimeout`. Logging and `SendInput` run on their own threads.
- Events we inject carry `output::MAGIC` in `dwExtraInfo`; all injected events are passed through untouched.
- Background and design discussion (Japanese) live in the user's Obsidian Vault:
  `01_プロジェクト/開発アイディア/nubmiddle/`.
