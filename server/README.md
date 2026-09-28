# jbehave-language-server

Standalone LSP server (`tower-lsp-server`) backing the Zed extension:
indexes `@Given`/`@When`/`@Then` step definitions in `.java`/`.groovy`
files and resolves `.story` steps for go-to-definition + diagnostics.

## Local development (no release needed)

The extension prefers a binary already on `$PATH` (`worktree.which`
in `src/lib.rs`), so for local work just build and link it:

```sh
cargo build --release -p jbehave-language-server
ln -sf "$PWD/target/release/jbehave-language-server" ~/.local/bin/
```

Then restart Zed (or `project: restart language servers`). The server logs
`jbehave-language-server: indexed N step definition(s)` on startup.

## Releases (how the extension self-installs the server)

With no binary on `$PATH`, the extension calls
`latest_github_release("HenrikNilsson/zed-jbehave")` and downloads the
asset named by `asset_name_for` in `src/lib.rs`:

| Runner / target                        | Asset name (exact)                              |
| -------------------------------------- | ----------------------------------------------- |
| `ubuntu-latest` (x86_64)               | `jbehave-language-server-linux-x86_64.tar.gz`   |
| `ubuntu-24.04-arm` (aarch64)           | `jbehave-language-server-linux-aarch64.tar.gz`  |
| `macos-latest` (aarch64)               | `jbehave-language-server-macos-aarch64.tar.gz`  |
| `windows-latest` (x86_64)              | `jbehave-language-server-windows-x86_64.zip`    |

Each archive must contain the `jbehave-language-server` binary
(`.exe` on Windows) at its **top level** — after extraction the extension
expects it at `<version-dir>/jbehave-language-server`.

To publish: push a tag (`git tag v0.1.0 && git push origin v0.1.0`).
`.github/workflows/release.yml` builds all four natively (no cross
toolchains) and attaches the archives to that tag's release. Keep the
matrix and `asset_name_for` in sync — a renamed asset means a 404 and a
dead server for that platform (visible in Zed's logs as
`Failed to start language server "jbehave-language-server"`).

Not covered by the matrix: 32-bit x86 and Intel macs. Users there need
the `$PATH` workaround above (or extend the matrix with a matching runner).
