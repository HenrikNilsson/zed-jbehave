# Parity checklist

Tracks this extension against the feature set of IntelliJ's JBehave plugins
(`kumaraman21/IntelliJBehave`, JetBrains Marketplace "JBehave Support
Extension"). Each item links to the test(s) that prove it, so "done" means
verifiably done, not just implemented.

| Feature | Status | Proof |
|---|---|---|
| `.story` syntax highlighting | Partially done | `tree-sitter-jbehave` (separate repo) is done: 29/29 corpus tests passing, `queries/highlights.scm` present and copied to [`languages/jbehave/highlights.scm`](../languages/jbehave/highlights.scm). Zed-side install is unblocked — `extension.toml`'s `[grammars.jbehave]` points at the published repo (`github.com/HenrikNilsson/tree-sitter-jbehave`) — but highlighting still needs to be eyeballed in a running Zed. |
| Scenario outline/breadcrumbs | Partially done | [`languages/jbehave/outline.scm`](../languages/jbehave/outline.scm) written (`scenario`/`narrative`/`meta`/`examples_table` → `@context`/`@name`). Queries are installable now (same `rev`/remote fix as highlighting); verify in Zed. |
| Comment/uncomment `.story` lines | Partially done | [`languages/jbehave/config.toml`](../languages/jbehave/config.toml) written with `line_comments = ["!-- "]`. Queries are installable now (same `rev`/remote fix as highlighting); verify in Zed. |
| Jump to Java/Groovy step definition (Ctrl/Cmd+click, Ctrl/Cmd+B) | Done | End to end: step→method resolution in [`server/src/matcher/mod.rs`](../server/src/matcher/mod.rs) + [`server/src/index/mod.rs`](../server/src/index/mod.rs); `.story` buffer parsing in [`server/src/story/mod.rs`](../server/src/story/mod.rs); `textDocument/definition` in [`server/src/lsp/mod.rs`](../server/src/lsp/mod.rs). Proven by [`server/tests/definition.rs`](../server/tests/definition.rs), an integration test driving the real `tower-lsp-server` service against `test-fixtures/sample-project` (asserts a `Scalar` location, `Array` for ambiguous, `null` for undefined, and that a step referenced via `@Alias` resolves). |
| "Undefined step" diagnostic | Done | [`server/src/lsp/mod.rs`](../server/src/lsp/mod.rs) re-parses a changed `.story` file and publishes "Undefined step" (Error) / "Ambiguous step: matches N step definitions" (Warning) diagnostics. Proven by [`server/tests/definition.rs`](../server/tests/definition.rs) (asserts the published `publishDiagnostics` message). |
| "Unused step declaration" inspection | Partially done | Core logic implemented and tested against synthetic usage lists: [`server/src/analysis/unused_steps.rs`](../server/src/analysis/unused_steps.rs). The story parser now exists, so real usages are collectable; and since `extension.toml` now attaches the server to Java/Groovy as well, pushing diagnostics into `.java` files should work — what's left is wiring story parsing across the workspace into `find_unused` and publishing the results. |
| Step completion | Not started | Planned for `server/src/lsp/completion.rs`. |
| Java step-definition discovery (`@Given/@When/@Then/@Alias/@Aliases`) | Done | [`server/src/java/scanner.rs`](../server/src/java/scanner.rs), tests in the same file. |
| Groovy step-definition discovery | Done | [`server/src/java/groovy_scanner.rs`](../server/src/java/groovy_scanner.rs); verified `tree-sitter-groovy` mirrors `tree-sitter-java`'s node kinds via a scratch parse-tree dump before committing to shared logic in [`server/src/java/common.rs`](../server/src/java/common.rs). |
| Alias resolution (`@Alias`/`@Aliases` treated as equivalent patterns) | Done | [`server/src/matcher/mod.rs`](../server/src/matcher/mod.rs) `aliases_of_the_same_method_are_not_ambiguous`; step-type inheritance in [`server/src/index/mod.rs`](../server/src/index/mod.rs) `alias_inherits_step_type_from_sibling_annotation`. |
| Ambiguous-step detection (mirrors JBehave's `DuplicateCandidateFoundException`) | Done | [`server/src/matcher/mod.rs`](../server/src/matcher/mod.rs) `distinct_methods_matching_the_same_text_are_ambiguous_with_best_guess`, `true_tie_has_no_best_guess`. |
| New-file templates/wizard | Not started | Needs verification of whether Zed's extension API has an equivalent to IntelliJ's file-template wizard at all (may end up as a snippet/task instead). |
| Rename across alias sets | Not started | Planned for `server/src/lsp/rename.rs`. |
| Multiline step text | Partially done | Story side done: `story::parse` joins `step_continuation_line`s into the step text (see [`server/src/story/mod.rs`](../server/src/story/mod.rs)). Matcher side still missing: `matcher::compile_pattern` doesn't dotall-match `\n` yet, so a multiline step with a `$param` spanning lines won't resolve. |
| Configurable non-`$` parameter prefix | Done | Set via `"lsp": { "jbehave-language-server": { "settings": { "parameter_prefix": "%" }}}`; the extension forwards the settings block (`language_server_workspace_configuration` in [`src/lib.rs`](../src/lib.rs)) and the server applies it on `workspace/didChangeConfiguration` ([`server/src/lsp/mod.rs`](../server/src/lsp/mod.rs)), matching/specificity then honor each definition's own prefix ([`server/src/matcher/mod.rs`](../server/src/matcher/mod.rs) `custom_prefix_*`, [`server/src/index/mod.rs`](../server/src/index/mod.rs) `custom_placeholder_prefix_*`). Proven end to end by `changing_parameter_prefix_via_configuration_updates_resolution_and_diagnostics` in [`server/tests/definition.rs`](../server/tests/definition.rs). Syntax highlighting still assumes `$` (grammar is compiled per prefix); documented as a limitation, not worth a grammar rebuild. |

## Explicit non-goals (v1)

- **Step definitions in dependency `.jar`s.** Discovery only scans workspace
  source files (any `.java`/`.groovy` file under the project root, respecting
  `.gitignore` — see `index_workspace` in
  [`server/src/lsp/mod.rs`](../server/src/lsp/mod.rs)). A step defined only in
  a compiled dependency won't resolve. IntelliJ's plugin can do this because
  it piggybacks on IntelliJ's own project/classpath model; replicating that
  from scratch is out of scope unless it turns out to matter in practice.
