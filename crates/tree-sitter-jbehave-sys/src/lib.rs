//! Rust bindings for the vendored `tree-sitter-jbehave` grammar.
//!
//! The grammar is developed at the sibling `~/Code/tree-sitter-jbehave` repo,
//! which has no `bindings/rust` of its own and is read-only from here. The C
//! sources are therefore vendored into `vendor/src/` and compiled by
//! `build.rs` (mirroring how `tree-sitter-java`'s crate is laid out, plus the
//! external `scanner.c`, like `tree-sitter-groovy`).
//!
//! Because this is vendored by hand, the C sources must be re-copied manually
//! if the sibling repo's grammar ever changes; the two can't drift silently —
//! this crate's `LANGUAGE` constant is the one the language server parses
//! `.story` files with, so a stale copy shows up immediately as story-parse
//! test failures.

use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_jbehave() -> *const ();
}

/// The tree-sitter [`LanguageFn`][LanguageFn] for this grammar.
///
/// [LanguageFn]: https://docs.rs/tree-sitter-language/*/tree_sitter_language/struct.LanguageFn.html
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_jbehave) };

/// The content of the [`node-types.json`][] file for this grammar.
///
/// [`node-types.json`]: https://tree-sitter.github.io/tree-sitter/using-parsers#static-node-types
pub const NODE_TYPES: &str = include_str!("../vendor/src/node-types.json");

#[cfg(test)]
mod tests {
    /// Every Zed query file must still match the vendored grammar: a renamed
    /// node in a future re-vendor would otherwise silently break
    /// highlighting/outline/folds/runnables. `Query::new` fails on unknown
    /// node names, so this pins the queries to the grammar. `include_str!`
    /// (rather than reading at runtime) keeps this working under
    /// `make test-wasm`, where the sandbox has no filesystem access.
    #[test]
    fn zed_queries_match_the_grammar() {
        let language: tree_sitter::Language = super::LANGUAGE.into();
        let queries = [
            (
                "highlights.scm",
                include_str!("../../../languages/jbehave/highlights.scm"),
            ),
            (
                "outline.scm",
                include_str!("../../../languages/jbehave/outline.scm"),
            ),
            (
                "folds.scm",
                include_str!("../../../languages/jbehave/folds.scm"),
            ),
            (
                "indents.scm",
                include_str!("../../../languages/jbehave/indents.scm"),
            ),
            (
                "runnables.scm",
                include_str!("../../../languages/jbehave/runnables.scm"),
            ),
        ];
        for (file, source) in queries {
            tree_sitter::Query::new(&language, source)
                .unwrap_or_else(|e| panic!("invalid query in {file}: {e}"));
        }
    }

    #[test]
    fn test_can_load_grammar() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&super::LANGUAGE.into())
            .expect("Error loading JBehave parser");
    }

    /// Smoke-test that the vendored grammar parses a representative story
    /// into the node shapes `story::mod` depends on: `source_file` →
    /// `scenario` → `step` (`step_keyword` + `step_text`).
    #[test]
    fn parses_a_story_into_expected_node_shapes() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&super::LANGUAGE.into())
            .expect("Error loading JBehave parser");
        let source = "Scenario: has balance\nGiven Bob has $amount in account\nWhen Bob withdraws $amount\nThen balance is $amount\n";
        let tree = parser.parse(source, None).expect("parse should succeed");
        assert!(!tree.root_node().has_error(), "tree:\n{tree:#?}");

        let root = tree.root_node();
        assert_eq!(root.kind(), "source_file");

        let scenario = root
            .children(&mut root.walk())
            .find(|c| c.kind() == "scenario")
            .expect("a scenario node");
        assert_eq!(
            scenario
                .child_by_field_name("title")
                .map(|t| &source[t.byte_range()]),
            Some("has balance")
        );

        let steps: Vec<_> = scenario
            .children(&mut scenario.walk())
            .filter(|c| c.kind() == "step")
            .collect();
        assert_eq!(steps.len(), 3);
        let first_keyword = steps[0]
            .child(0)
            .expect("step_keyword")
            .child(0)
            .expect("given_kw");
        assert_eq!(first_keyword.kind(), "given_kw");
        let first_text = steps[0]
            .children(&mut steps[0].walk())
            .find(|c| c.kind() == "step_text")
            .expect("step_text");
        assert_eq!(
            &source[first_text.byte_range()],
            "Bob has $amount in account"
        );
    }
}
