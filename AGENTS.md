# AGENTS.md

This file is the agent-facing brief for this repo: current state, hard
constraints, and what's left to build. It's meant to be self-contained — if
you're an agent picking this up cold, you shouldn't need anything beyond this
file, `README.md`, and `docs/PARITY.md`. Keep this file's "Current state"
section up to date as work lands; it's a living status doc, not a one-time
snapshot.

For the human-facing pitch (what this is, why it exists), see `README.md`.
This file is about *how to work on it*.

## Hard constraints

- **`~/Code/tree-sitter-jbehave` is a sibling repo and is read-only from
  here.** Never write to it, never run `git` commands in it, never assume
  you can add files there. If something seems to require a change there
  (e.g. adding Rust bindings), find a way to do it from `zed-jbehave`'s side
  instead (see "Consuming the grammar" below for a worked example of this).
- **Git in both repos is managed manually by the project owner.** Don't run
  `git init`/`add`/`commit`/`push` in either repo unless explicitly asked to.
  Neither repo has git history yet as of this writing.
- Before running anything that installs packages, needs `sudo`, or otherwise
  changes the system (e.g. `apt-get install`), ask first.
- `~/.cargo/bin` is not guaranteed to be on `PATH` in every shell — if `cargo`
  isn't found, that's why; check with the user before assuming it's missing
  entirely.

## Current state

Three-crate Cargo workspace:

- `./` (the workspace root package) — the Zed extension. **Done**:
  implements `zed::Extension::language_server_command`, which prefers a
  `jbehave-language-server` binary already on `$PATH` (local dev) and
  otherwise downloads a matching release asset from GitHub (`HenrikNilsson/
  zed-jbehave` — see `SERVER_REPO` constant), plus `language_server_workspace_configuration`, which forwards the user's
  `"lsp": { "jbehave-language-server": { "settings": {...} } }` block to the
  server (Zed pushes it as `workspace/didChangeConfiguration` right after
  initialize — the server uses it for e.g. a custom `parameter_prefix`).
  **Important layout constraint**: Zed's `zed: install dev extension` builds
  the extension in the directory it's pointed at, runs
  `cargo build --target wasm32-wasip2` there, parses the root `Cargo.toml`
  for `package.name`, and reads the component from
  `target/wasm32-wasip2/{debug,release}/zed_jbehave.wasm`. That's why the
  extension crate is the workspace **root package** (`Cargo.toml` has both
  `[package]` and `[workspace]`, `src/lib.rs` at the repo root) — it can't be
  nested under `crates/`. The wasm target is `wasm32-wasip2` (not wasip1).
- `crates/tree-sitter-jbehave-sys/` — **done, tested.** The tree-sitter-jbehave
  grammar vendored as a Rust crate (C sources copied from the sibling repo,
  compiled by `build.rs` — see "Consuming the grammar" below for why it's
  vendored and how to re-sync).
- `server/` — `jbehave-language-server`, a standalone native
  `tower-lsp-server` server (the maintained community fork of the dormant
  `tower-lsp`; its `ls_types` module replaced upstream's `lsp_types`, with
  `Url` becoming a fluent-uri-backed `Uri`). All crates are on edition 2024.
  Status by module:
  - `matcher/` — **done, tested.** Compiles JBehave `$param` step patterns
    into anchored regexes, resolves ambiguity (mirrors JBehave's own
    `DuplicateCandidateFoundException` semantics: multiple matching methods
    → `Ambiguous` with a specificity-based `best_guess`, never a silent
    guess). The placeholder prefix is configurable per `StepDefinition`
    (default `$`, swapped workspace-wide via the server's `parameter_prefix`
    setting), and specificity only counts placeholders under the prefix in
    effect (`matcher::custom_prefix_*` tests).
  - `java/` — **done, tested.** Scans `.java`/`.groovy` source (via
    `tree-sitter-java`/`tree-sitter-groovy`, not regex — regex breaks on text
    blocks, multi-line annotations, strings-in-comments) for
    `@Given/@When/@Then/@Alias/@Aliases`-annotated methods. Java and Groovy
    scanning share one implementation in `java/common.rs`, since
    `tree-sitter-groovy`'s node kinds/field names mirror Java's closely
    enough (verified by dumping a sample parse tree before committing to
    this).
  - `index/` — **done, tested.** `StepDefinitionIndex`: per-file incremental
    index (each `update_java_file`/`update_groovy_file` call fully replaces
    that file's entries), resolves alias step-type inheritance, cross-file
    ambiguity, and holds a workspace-wide `parameter_prefix` that re-runs
    pattern matching for every definition on the next `resolve` (definitions
    are compiled on demand, so flipping the prefix needs no reindex).
  - `story/` — **done, tested.** Parses `.story` buffers via the vendored
    grammar: `parse` walks `source_file` → `scenario` → `step`, `and_kw`
    inherits the prior explicit `Given`/`When`/`Then` within each scenario,
    step text is the `step_text` slice verbatim plus `\n`-joined
    `step_continuation_line`s (trailing whitespace trimmed, `table_row`s
    excluded), and `step_at` hit-tests an LSP position. Lifecycle
    `Before:`/`After:` steps aren't collected yet (flagged in PARITY.md).
  - `analysis/unused_steps.rs` — **logic done, tested against synthetic
    data.** `find_unused` needs *real* story-derived usages to be useful;
    wiring it to `story::parse` across the workspace is still a later phase.
  - `lsp/` — **done for go-to-definition + step diagnostics + workspace
    config.** `Backend`
    implements `initialize` (scans the workspace for `.java`/`.groovy` files
    and indexes them, off-thread via `spawn_blocking`),
    `did_open`/`did_change`/`did_save` (update the index and keep `.story`
    text + client version in a `documents` map, re-publishing "Undefined
    step"/"Ambiguous step" diagnostics after each story change, tagged with
    that version so they can't be clobbered by stale replies),
    `did_close` (drops the stored story text and clears its diagnostics),
    `did_change_configuration` (parses the user's settings — currently the
    optional `parameter_prefix` char — applies it to the index, and
    re-publishes diagnostics for every open `.story` since a prefix change
    re-runs matching),
    and `goto_definition` (resolves the step under the cursor via the index,
    returning `Scalar(Location)` for a unique match and
    `Array(candidates)` when ambiguous).
  - **Multi-language attach:** `extension.toml` declares
    `languages = ["JBehave", "Java", "Groovy"]`, the same mechanism Emmet
    uses — Zed forwards Java/Groovy document events to this one shared
    server instance, so step-definition edits are indexed live instead of
    requiring a restart. Proven end-to-end by
    `editing_a_java_file_updates_step_resolution_without_reinitialize`.

`languages/jbehave/{config.toml,highlights.scm,brackets.scm,indents.scm,
outline.scm,folds.scm,runnables.scm}` are populated — copied from
`tree-sitter-jbehave`'s `queries/` once that grammar was finished (29/29
corpus tests passing there, including `and_keyword_continuation.txt` which
pins single-letter `And` keywords (`I`/`E`/`Y`) as winning over continuation
lines — corpus-only addition, so the vendored C here is unaffected).
`extension.toml`'s `[grammars.jbehave]` points at the published repo
(`repository = "https://github.com/HenrikNilsson/tree-sitter-jbehave"`, pinned
to a `rev`). It does **not** affect the server: the language server consumes
the grammar via a Rust crate directly, never through Zed's extension-grammar
loading path.

Test suite: 53 passing tests — 47 in `server/` (unit) + 3 end-to-end
(`server/tests/definition.rs`, drives the real `tower-lsp-server` service against
`test-fixtures/sample-project`) + 3 in `crates/tree-sitter-jbehave-sys/`
(grammar load + parse-shape smoke test + queries-match-grammar). Run with
`make test`. Nothing in the extension crate (workspace root package) has tests
of its own; verify it with `make check` / `cargo check -p zed-jbehave --target
wasm32-wasip2`.

Grammar OOM fix (pinned `rev 4199c76`): the external scanner used to emit a
zero-width `EOL` at EOF, so any `repeat($._eol)` state at EOF shifted
synthetic EOLs forever without advancing — opening any `.story` in Zed ate
all memory until it died, while headless parses of complete stories were
unaffected (corpus inputs all end on content lines). Fixed upstream with a
`!saw_newline` guard at both EOF sites in `scanner.c`; the vendored copy
here is byte-identical to upstream's (per the `build.rs` convention — verify
with `diff` against the sibling, don't hand-edit drift back in). Regression
tests on both sides: `eof_edge_cases.txt` upstream (empty/blank/header-at-EOF
terminate) and `story::degenerate_inputs_terminate_with_no_steps` in
`server/` (the server hung the same way on empty `.story` buffers).

## Consuming the grammar (why it's vendored)

`tree-sitter-jbehave` has no `bindings/rust` (no root `Cargo.toml`, no
`bindings/` dir at all — just `src/parser.c`, `src/scanner.c`,
`src/node-types.json`, `src/grammar.json`). Rather than rely on it growing
bindings (it's read-only from here) or adding a
`path = "../../../tree-sitter-jbehave"` dependency (works locally, breaks any
CI that only checks out `zed-jbehave`), the C sources are vendored into
`crates/tree-sitter-jbehave-sys/vendor/src/` and compiled by `build.rs`.

**This means the vendored copy needs manual re-sync if the sibling repo's
grammar changes** — `build.rs` has a comment to that effect. The grammar is
generated at ABI 15 (the current `tree-sitter` CLI), which is why `server/`
and the sys crate use `tree-sitter` **0.25** (the first Rust crate whose
supported ABI range 13..=15 covers it); do not downgrade it to 0.24 (ABI 14
max) or the grammar stops loading with `LanguageError { version: 15 }`.

## Next up (later phases)

- Feeding `analysis::unused_steps` real usages: `story::parse` is now
  available, and the multi-language attach means `.java` diagnostics should
  surface — wiring it up is now just collecting usages across the
  workspace's stories and publishing the findings.
- Lifecycle `Before:`/`After:` steps aren't collected by `story::parse`
  (only `scenario` → `step` is walked); collecting them means carrying the
  "current step type" across each `lifecycle_section` boundary too.
- Completion, rename, multiline steps, new-file templates.
- Manual verification in a real running Zed (`zed: install dev extension`,
  confirm highlighting + go-to-definition) — can't be done headlessly from
  an agent session; this is on the project owner.

## Commands

```sh
make            # no target: lists every available target with its description
make test       # cargo test -p jbehave-language-server + -p tree-sitter-jbehave-sys
make check      # cargo check on the workspace (fast, no codegen)
make build      # release build of both: server binary + extension .wasm
```

## Where to look for more detail

- `docs/PARITY.md` — feature-by-feature status against IntelliJ's JBehave
  plugins, each row linking to what actually proves it.
- `README.md` — project pitch, architecture rationale, prerequisites,
  install-as-dev-extension steps.
