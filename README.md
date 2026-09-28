# zed-jbehave

[![ci](https://github.com/HenrikNilsson/zed-jbehave/actions/workflows/ci.yml/badge.svg)](https://github.com/HenrikNilsson/zed-jbehave/actions/workflows/ci.yml)
[![release-server](https://github.com/HenrikNilsson/zed-jbehave/actions/workflows/release.yml/badge.svg)](https://github.com/HenrikNilsson/zed-jbehave/actions/workflows/release.yml)

A [Zed](https://zed.dev) extension bringing [JBehave](https://jbehave.org)
(`.story` file) support to the editor, aiming for feature parity with
IntelliJ's JBehave plugins: syntax highlighting, jump-to-definition from a
story step to its Java/Groovy `@Given`/`@When`/`@Then` method, undefined- and
unused-step diagnostics, and step completion.

**Status:** early but moving — the language server now parses `.story`
buffers and powers go-to-definition and "undefined"/"ambiguous step"
diagnostics end to end (proven by an integration test driving the real
`tower-lsp-server` service). See [`docs/PARITY.md`](docs/PARITY.md) for the
feature-by-feature checklist against the IntelliJ plugins, with links to
what's actually tested, and [`AGENTS.md`](AGENTS.md) for a fuller
current-state brief. The [`tree-sitter-jbehave`](#the-grammar-lives-in-a-separate-repo)
grammar itself is done (29/29 corpus tests passing) and is consumed by the
server via the vendored `tree-sitter-jbehave-sys` crate.

## How it's put together

Three Rust crates in one Cargo workspace:

```
zed-jbehave/
├── extension.toml                  # Zed extension manifest: grammar + language-server registration
├── Cargo.toml                      # workspace root = the extension crate (compiles to wasm32-wasip2)
├── src/lib.rs                      #   downloads/launches the language server binary
├── crates/tree-sitter-jbehave-sys/ # the tree-sitter-jbehave grammar vendored as a Rust crate
├── languages/jbehave/              # tree-sitter query files (highlights/outline/brackets/indents) for Zed's editor UI
├── server/                         # jbehave-language-server: a standalone, native LSP server
│   └── src/
│       ├── matcher/                #   compiles $param step patterns into regexes, resolves ambiguity
│       ├── java/                   #   scans .java/.groovy source for @Given/@When/@Then/@Alias/@Aliases
│       ├── index/                  #   StepDefinitionIndex: per-file incremental step-definition index
│       ├── story/                  #   parses .story buffers via the vendored grammar
│       ├── analysis/               #   "unused step declaration" inspection logic
│       └── lsp/                    #   the tower-lsp-server (community fork) server: workspace scanning, textDocument/definition, diagnostics
├── test-fixtures/sample-project/   # fixture Java step class + .story files used by the server's tests
├── examples/                       # sample tasks.json files binding the play button (Maven/Gradle)
└── docs/PARITY.md                  # feature-by-feature status vs. the IntelliJ plugins
```

**Why a native language server?** Zed extensions run sandboxed in WebAssembly
and have no way to inspect Zed's own parse trees of *other* files/languages.
Resolving a story step's Java method — or reporting diagnostics that span both
a `.story` file and the `.java` files that define its steps — has to happen in
a real, native language server process that the extension downloads and
launches. This is the same architecture used by
[`@cucumber/language-server`](https://www.npmjs.com/package/@cucumber/language-server)
for Gherkin/Cucumber tooling; `jbehave-language-server` is this project's
equivalent, written in Rust instead of Node so the extension can ship a
prebuilt binary with no extra runtime dependency.

### The grammar lives in a separate repo

Zed's `extension.toml` fetches tree-sitter grammars via a `repository` + `rev`
pointing at a git repo — it can't reference a subdirectory of this repo. The
grammar is developed at a sibling repo, `tree-sitter-jbehave`, referenced
from `extension.toml`'s `[grammars.jbehave]` table. **That grammar is done**
(29/29 corpus tests passing, has its own `queries/{highlights,folds,
runnables}.scm`) and is treated as read-only from this repo going forward —
its output has already been copied into `languages/jbehave/`. For local
development the grammar is pinned to the local checkout's commit via a
`file://` URL in `extension.toml` (Zed clones it with git at install time);
before anyone else can install the extension, that must become a real remote
`repository` + `rev`. Either way it only matters for Zed's
syntax-highlighting path — the language server doesn't go through Zed's
grammar loading at all, it consumes the grammar via
`crates/tree-sitter-jbehave-sys`, which vendors the grammar's generated C
sources (see [`AGENTS.md`](AGENTS.md) → "Consuming the grammar").

## Prerequisites

- Rust (stable) — a [`rust-toolchain.toml`](rust-toolchain.toml) pins the
  channel and the `wasm32-wasip2` target (what Zed builds dev extensions
  with), so `rustup` installs both automatically the first time you build.
- A C toolchain (`cc`/`gcc`) — needed by some of the server's dependencies'
  build scripts. On Debian/Ubuntu: `sudo apt-get install build-essential`.

## Building and testing

```sh
make            # no target: lists every available target with its description
make test       # runs the language server's test suite (unit + integration)
make check      # cargo check on the workspace (fast, no codegen)
make build      # release build of both: server binary + extension .wasm
```

Or drive `cargo` directly:

```sh
cargo test -p jbehave-language-server
cargo check -p zed-jbehave --target wasm32-wasip2
cargo build --release -p jbehave-language-server
cargo build --release -p zed-jbehave --target wasm32-wasip2
```

The extension crate (workspace root) and the native server crate are kept as
separate workspace members deliberately: the server pulls in
`tokio`/`tower-lsp-server`, which must never end up in the extension's
`wasm32-wasip2` build graph. Zed's dev-extension install runs a plain
`cargo build --target wasm32-wasip2` in the repo root, which only builds the
root package (the extension) — the server and grammar crates are workspace
members that are never part of that build.

## Installing as a dev extension in Zed

1. Build the server and make it available on `$PATH`:
   `cargo build --release -p jbehave-language-server` then put
   `target/release/jbehave-language-server` somewhere on your `$PATH` (the
   extension prefers a `jbehave-language-server` binary on `$PATH`; the
   GitHub-release fallback needs `SERVER_REPO` to point at a real repo with
   published assets, which doesn't exist yet).
2. In Zed, open the command palette and run `zed: install dev extension`,
   then select this repository's root directory.

On first install Zed compiles the Rust extension and fetches + compiles the
grammar (from `extension.toml`'s pinned `file://` commit). Highlighting,
go-to-definition, and undefined/ambiguous-step diagnostics have not yet been
eyeballed in a real running Zed — see [`docs/PARITY.md`](docs/PARITY.md) for
status on what actually works versus what's wired up but untested.

## Running scenarios (the play button)

The extension's `languages/jbehave/runnables.scm` tags every `Scenario:` line
with `jbehave-scenario` and exposes the scenario title as `$ZED_CUSTOM_NAME`.
Zed only draws the gutter play button once a **task binds to that tag** — and
the command is project-specific (only you know how your stories launch), so it
lives in your config, not in the extension. Starting points are in
[`examples/`](examples/) (`tasks-maven.json`, `tasks-gradle.json`); copy one
into your global `~/.config/zed/tasks.json` (`zed: open tasks`) or the
project's `.zed/tasks.json` (`zed: open project tasks`).

Variables available to the command: `$ZED_FILE` (the story), `$ZED_CUSTOM_NAME`
(scenario title), `$ZED_ROW` (button row), `$ZED_WORKTREE_ROOT`. Start with the
included echo task to prove the wiring (a terminal panel prints the resolved
values), then adapt the run task. Note the samples run the whole module suite:
narrowing to one scenario depends on your runner (JUnit runner class, Maven
Failsafe includes, JBehave meta filters, …) — the extension hands you
file+title+row and stays out of the way.

## Contributing

This project is being built in phases. If you're picking up work here
(including as an AI coding agent), start with [`AGENTS.md`](AGENTS.md) — it
has the current architecture snapshot, hard constraints (e.g. the grammar
repo is read-only), and the concrete next steps. [`docs/PARITY.md`](docs/PARITY.md)
tracks feature-by-feature status against the IntelliJ plugins this aims for
parity with.

## License

[MIT](LICENSE)
