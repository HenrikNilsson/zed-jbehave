# Investigating the zed-editor OOM on dev-extension (re)load

Status doc for an in-flight investigation (2026-08-09). Follow the repro
protocol below; the harness + monitor scripts make the leak safe to reproduce.

## What we know (from `~/.local/share/zed/logs/Zed.log`, 2026-08-09)

- Two OOMs, both timed exactly to a dev-extension (re)load which forces Zed to
  stop/restart every LSP server (jdtls, clangd, eslint, vtsls, ...):
  - **Crash 1 (10:32:39):** `zed: install dev extension` completed (35s build),
    then Zed ran `checking out jbehave parser` / `compiling jbehave parser`
    (the grammar compiled at load time from the `file://` URL in
    `extension.toml`), then `extensions updated. loading 1` → all servers
    stopped/restarted. `zed-editor` hit 12.6GB by +12s, 44GB by +40s, kernel
    OOM ~10:33:30.
  - **Crash 2 (10:34:08):** an auto-reload 3s after a clean start (no
    recompile; trigger unknown) → servers stopped (jdtls was NOT one of them) →
    55GB by +27s.
  - **Stable (10:36:56):** clean start, extension already loaded, no reload →
    770MB for 1h+. Zero repro.
- The growing process is `zed-editor` itself (its own `zed::reliability` memory
  samples prove it). Growth is log-silent, roughly linear at 1-2 GB/s.
- No `.story` buffer was open at crash time, so it is not grammar-parsing a
  story file. No ABI mismatch: the extension loads, `language_server_command`
  runs, and the vendored grammar parses fine (tree-sitter ABI 15 / 0.25).
- The extension crate's entire runtime surface is `language_server_command`
  (`src/lib.rs`); it cannot loop or allocate during the crash.
- What IS unusual about this setup vs. shipping extensions:
  1. The grammar is compiled by Zed **at load time from a `file://` URL**
     pointing at `~/Code/tree-sitter-jbehave`, which is itself an **open
     project** in the same Zed (every LSP's cwd was `tree-sitter-jbehave`).
  2. Zed writes `extension.wasm`, `grammars/`, `target/` **inside the open
     `zed-jbehave` project**, i.e. build artifacts land in directories Zed's
     worktree/git watchers are watching.
  3. The opencode ACP bridge runs inside zed (this session's ancestor chain is
     `opencode acp` → `zed-editor`).

Open question: the growth window is log-silent, so "reload → LSP teardown →
OOM" is correlation. The allocating function has not been identified; the
experiments below exist to name it.

## Harness (contain the leak)

- `scripts/zed-harness.sh` — launches a fresh `zed-editor` under a systemd user
  scope with `memory.max` (default 10G, `memory.high` 8G, swap 0). The kernel
  OOM-kills the ballooning process **inside the cgroup only**; the machine is
  never at risk. Configure via `ZED_HARNESS_MEM_MAX` / `ZED_HARNESS_MEM_HIGH`.
- `scripts/zed-mem-watch.sh` — samples every pid in the harness cgroup every
  0.2s to `/tmp/zed-mem-watch/rss.log`; snapshots `/proc/<pid>/smaps`,
  per-thread `/proc/<pid>/task/*/stat`, and a `ps --forest` tree at
  2/4/6/8/10G crossings and on >1G-per-tick growth; on death reports cgroup
  `memory.peak` and `memory.events` (the `oom_kill` counter proves the cap
  worked).

## Repro protocol

1. **Quit ALL running zed instances.** Do not run two instances side by side:
   they share `~/.local/share/zed`, and a dev-extension reload in one can fire
   in the other (which would be uncapped → machine OOM again). The opencode
   panel reconnects after restart; the full protocol lives in this file.
2. `./scripts/zed-harness.sh` — confirm it prints `harness live` and a window
   opens. (If it prints the handoff warning, an old instance was still running.)
3. In the harnessed zed: open the `fortnoxorder` project.
4. From a terminal: `./scripts/zed-mem-watch.sh` (leave it running).
5. Trigger the known repro: `zed: install dev extension` →
   `/home/henrik.nilsson@fnox.it/Code/zed-jbehave`. Expect ~35s of build +
   grammar compile, then the reload.
   - Crash-2-style alternative: just keep the `zed-jbehave` repo open in the
     harnessed zed — the dev-extension watcher may auto-reload on its own
     (writing `extension.wasm`/`grammars/` back into the watched repo).
6. The harnessed zed should balloon and be OOM-killed at 10G within ~60s. The
   machine stays up. The monitor reports the cgroup `oom_kill`.

## Reading the evidence (bring back to the opencode session)

- `rss.log`: which pid grows and at what rate (zed-editor vs jdtls `java` vs a
  `node` server).
- `captures/<ts>/context.txt`: full process tree + cgroup `memory.peak` /
  `memory.events`.
- `smaps/`: a giant anonymous private region → heap/allocator; wasmtime linear
  memory shows up as large `PROT_NONE`-backed anon mappings; file-backed growth
  → log/buffer capture.
- `threads/`: `utime`/`stime` per thread across snapshots → which thread spins
  (`main` vs `tokio-runtime-worker` vs `background`).
- zed's own `~/.local/share/zed/hang_traces/*.miniprof.json` created during the
  growth name the busy task (startup hang traces already showed
  `extension_host/src/wasm_host.rs:649` and
  `language/src/language_registry.rs:634`).

## Cheap bisections (if the evidence is ambiguous)

- **A. Strip the grammar:** delete the `[grammars.jbehave]` block in
  `extension.toml` + the `path_suffixes`/`grammar` lines in
  `languages/jbehave/config.toml` → repro. If it still OOMs, the grammar is
  exonerated.
- **B. Strip the language server:** remove `[language_servers.jbehave-language-server]`
  (keep grammar) → repro.
- **C. Smaller project:** repro with a tiny workspace instead of `fortnoxorder`
  (is the project size required?).
- **D. Smaller cap:** `ZED_HARNESS_MEM_MAX=4G` to shorten the growth window.

Outcome decides the fix: a zed bug (file upstream with this evidence), our
grammar (fix/remove the `file://` compile-at-load path), our server flooding
zed (fix the server), or the dev-extension reload path itself.

## Post-migration status (2026-08-25)

`tower-lsp` 0.20 → `tower-lsp-server` 0.23, `dashmap` 5 → 6, edition 2024,
tokio features slimmed, extension wasm rebuilt. The wasm component's runtime
surface is unchanged (`language_server_command` — one function, one call per
attach). Nothing in the server or extension can allocate inside `zed-editor`'s
heap, so the migration does not change the OOM theory.

One new variable: **Task 4 (multi-language attach)** means the server now
launches when any `.java` or `.groovy` file is open, not just `.story`. Before
this change, the server was never running during the OOM repros (no story
buffer was open). Its footprint is tiny (few MB, bounded index, transient
regex compiles), but when rerunning **bisect B**, this is now a real cut
where it previously was a no-op.

**Repro status:** not re-run since migration. The existing harness
(`scripts/zed-harness.sh` + `zed-mem-watch.sh`) is ready; nothing in the
repro protocol changed. If the OOM persists, bisect A (strip `file://`
grammar → push `tree-sitter-jbehave` to a real remote) remains the cleanest
permanent fix, as it eliminates the compile-at-load-from-open-project path
entirely.

## 2026-09-15: repro now pinned to story-open; extension is required; server & grammar exonerated

### What changed
- `extension.toml` now points at the **real remotes** (`[grammars.jbehave]`
  rev **`ce1749b3cc4a68dd293cf1aebd3e4b52e760d8df`** from
  `HenrikNilsson/tree-sitter-jbehave`, server downloads from `HenrikNilsson/
  zed-jbehave`); no more `file://` compile-at-load path. Zed still compiles the
  grammar C → wasm once at install (grammar wasm timestamped twice ~00:55/00:58).
- Repro is now a plain **story open**, not extension (re)load:
  - **00:58** crash 2GB→6GB in ~4s right at install/reload, instance `1103085`.
  - **01:03–01:04** crash 535MB→7.9GB in ~30s on story open, instance `1157611`,
    killed at 10G cap. Hot threads: `Worker-0` (tid 1157622) + `Worker-15`
    (tid 1157637), ~150 utime ticks / 1.9s ≈ 80% CPU each.
  - **01:28** crash 585MB→7.6GB on opening a SMALL story, instance `1187981`,
    killed at 10G cap. Confirmed twice: reopening the story after restart
    crashes again.
  - Current instance `1400966` stable ~555MB (no story open).

### Locked-in exonerations (this session)
1. **Language server + LSP attach path fully exonerated.** During the 01:28
   crash the process tree showed `jbehave-language-server` as
   `<defunct>` and it was **never respawned** — the crash reproduces with no
   server running at all. `did_open`/index/spawn theory dead.
   (This effectively re-runs bisect B *and* the multi-language-attach theory.)
2. **Grammar logic exonerated.** Vendored copy parses all 539 real fortnoxorder
   stories (largest 64KB) in ≤7ms; the **installed** (`ce1749b3`) C sources
   compile+parse all 539 in ≤2ms, 0 errors; all five `.scm` queries run within
   ms over every one (no >200ms run). Probes: repo example
   `crates/tree-sitter-jbehave-sys/examples/parse_probe.rs`; throwaway crate in
   `/tmp/opencode/grammar-probe/`.
3. **Extension is required** (bisect "no extension" already done by hand):
   + rename the installed extension aside,
   `~/.local/share/zed/extensions/installed/jbehave` → `jbehave.disabled`
   (symlink back to the repo) → open story: **no balloon and no highlighting**
   (unhighlighted confirms the extension really was absent). Trigger lives
   entirely inside zed's handling of OUR grammar/query path.
4. **Not a query-vs-grammar node-kind mismatch:** every node kind our `.scm`
   queries reference (`narrative`, `scenario`, `examples_table`, `lifecycle`,
   `given_kw`, ...) exists in the installed grammar's `node-types.json`.

### Where the balloon actually is (fresh smaps + miniprof vs old assumptions)
- smaps of the 7.9GB ballooning zed: ~7,515 MiB resident rw-p anonymous of
  ~7,840 MiB size in 64–128MB slabs → **glibc-style host heap**, NOT wasmtime
  linear memory (wasm linear memory = `PROT_NONE`-backed anon, 0 resident).
- miniprof hang traces from the 01:28 crash (`~/.local/share/zed/hang_traces/
  hang-2026-09-15_01-29-*.miniprof.json`): **Worker-1, Worker-8, Worker-10,
  Worker-15 parked 1.8s → 11.8s → 14.8s in `crates/gpui/src/app/context.rs:861`
  (`AppContext::with_window`)** across consecutive samples. Workers pile up
  behind a main thread that never yields = gpui task feedback loop (`cx.update`
  from a background task keeps re-entering the app context), not a simple CPU
  burn or a slow grammar. Earlier 01:04 traces showed the same two-worker
  pattern in the same function.
- User-visible symptom: highlighting works; **folding not active, no run
  button** (outline/folds/runnables queries produce no captures in zed). Could
  be the same root cause (a query zed executes in its wasm/sandbox path that
  loops or errors).

### Next steps (TODO)
- [ ] **Split grammar-wasm vs queries inside our path** — one decisive zed run
      with a STRIPPED extension copy (grammar + `config.toml` + empty `.scm`
      files): still crashes → grammar wasm; stable → reintroduce queries one
      at a time (highlights, then outline, folds, runnables) to find the
      culprit query.
- [ ] (Optional, ask first) repro the **wasm** pipeline in the shell with
      `tree-sitter` CLI + the already-compiled `grammars/jbehave.wasm`
      (node v24 + npm present) → parse+query the 539 stories in the sandbox
      without zed. Isolates were the loop can even be.
- [ ] (Optional, ask first — `apt-get install gdb` needs sudo) attach gdb to
      ballooning zed and catch the allocating frame instead of inferring from
      miniprof single-locations.
- [ ] If grammar-wasm is ruled in: test whether re-pinning `[grammars.jbehave]`
      to the OLDER rev (the vendored C, md5s below) changes behavior; compare
      `ce1749b3` lexer/scanner deltas.
        - installed `parser.c` = 2dd43899dfc331fcd584d132b2762f2b
        - installed `scanner.c` = cdb345ea3eb732f8d9bcee3896116bd2
        - vendored `parser.c` = 0f6d9de2248ab7264f3eaac7123a69db
        - vendored `scanner.c` = c9e478d65c6543e64c279447d787d36e
        (installed scanner adds a UTF-8 continuation byte-check; lexer tables
        differ. Vendored copy is older — needs re-vendor at some point.)
- [ ] Restore extension name afterwards: `jbehave.disabled` → `jbehave`.
- [ ] Separately (product bug, not OOM): outline/folds/runnables produce nothing
      in zed — investigate after the OOM or in parallel (fold/runnable queries
      may need adjusting).

### Crate-state warning for future sessions
Installed grammar on disk lives at
`~/.local/share/zed/extensions/installed/jbehave(\.disabled)?/grammars/jbehave/
src/` (a full read-only git checkout zed makes during install). Do not confuse
it with the repo-vendored copy in `crates/tree-sitter-jbehave-sys/vendor/src/`.
