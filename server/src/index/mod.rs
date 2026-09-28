use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use dashmap::DashMap;
use tower_lsp_server::ls_types::{Position, Range, Uri};

use crate::java::{RawStepDefinition, groovy_scanner, scanner};
use crate::matcher::{
    DEFAULT_PLACEHOLDER_PREFIX, MatchOutcome, StepDefinition, StepType, compile_pattern, resolve,
};

/// Indexes step definitions found in the workspace's Java/Groovy source
/// files, keyed per-file so a single file's changes only replace that file's
/// own entries rather than requiring a full workspace rescan.
pub struct StepDefinitionIndex {
    by_file: DashMap<Uri, Vec<RawStepDefinition>>,
    /// Bumped on every mutation (`update_*`, `remove_file`,
    /// `set_placeholder_prefix`); the snapshot is rebuilt only when its
    /// stored version lags. This keeps per-keystroke diagnostics at matching
    /// cost: the pre-cache design recompiled thousands of regexes per step
    /// per keystroke (~90s per 60-step pass on a 3000-definition workspace,
    /// debug profile).
    version: AtomicU64,
    snapshot: Mutex<(u64, Arc<Vec<StepDefinition>>)>,
    /// JBehave parameter prefix used when compiling patterns (default `$`),
    /// set via workspace configuration (`lsp.jbehave-language-server.settings
    /// .parameter_prefix`).
    placeholder_prefix: Mutex<char>,
}

impl Default for StepDefinitionIndex {
    fn default() -> Self {
        Self {
            by_file: DashMap::new(),
            version: AtomicU64::new(0),
            snapshot: Mutex::new((u64::MAX, Arc::new(Vec::new()))),
            placeholder_prefix: Mutex::new(DEFAULT_PLACEHOLDER_PREFIX),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ResolvedMatch {
    None,
    Unique(StepDefinition),
    Ambiguous {
        candidates: Vec<StepDefinition>,
        best_guess: Option<StepDefinition>,
    },
}

fn step_type_of(annotation: &str, siblings: &[RawStepDefinition]) -> Option<StepType> {
    match annotation {
        "Given" => Some(StepType::Given),
        "When" => Some(StepType::When),
        "Then" => Some(StepType::Then),
        // An alias has no step type of its own: it inherits whichever
        // Given/When/Then annotation appears on the same method.
        "Alias" | "Aliases" => siblings.iter().find_map(|d| match d.annotation.as_str() {
            "Given" => Some(StepType::Given),
            "When" => Some(StepType::When),
            "Then" => Some(StepType::Then),
            _ => None,
        }),
        _ => None,
    }
}

fn point_to_position(p: (usize, usize)) -> Position {
    Position::new(p.0 as u32, p.1 as u32)
}

fn build_step_definitions(
    file: &Uri,
    raw: &[RawStepDefinition],
    placeholder_prefix: char,
) -> Vec<StepDefinition> {
    let mut out = Vec::new();
    for def in raw {
        let siblings: Vec<RawStepDefinition> = raw
            .iter()
            .filter(|d| {
                d.declaring_class == def.declaring_class && d.method_name == def.method_name
            })
            .cloned()
            .collect();
        let Some(step_type) = step_type_of(&def.annotation, &siblings) else {
            continue;
        };
        let is_alias = def.annotation == "Alias" || def.annotation == "Aliases";
        let method_range = Range::new(
            point_to_position(def.method_start),
            point_to_position(def.method_end),
        );
        for pattern in &def.patterns {
            let Ok(compiled) = compile_pattern(pattern, placeholder_prefix) else {
                continue;
            };
            out.push(StepDefinition {
                step_type,
                pattern_source: pattern.clone(),
                compiled,
                placeholder_prefix,
                declaring_class: def.declaring_class.clone(),
                method_name: def.method_name.clone(),
                file: file.clone(),
                method_range,
                // TODO(Phase 4): track the annotation string literal's own
                // range separately once hover/rename need to target just the
                // pattern text rather than the whole method.
                literal_range: method_range,
                is_alias,
            });
        }
    }
    out
}

impl StepDefinitionIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_java_file(&self, uri: Uri, source: &str) {
        self.by_file.insert(uri, scanner::scan(source));
        self.version.fetch_add(1, Ordering::Relaxed);
    }

    pub fn update_groovy_file(&self, uri: Uri, source: &str) {
        self.by_file.insert(uri, groovy_scanner::scan(source));
        self.version.fetch_add(1, Ordering::Relaxed);
    }

    /// Sets the JBehave parameter prefix used when compiling patterns (e.g.
    /// a workspace configured with `parameter_prefix = "%"`). Applies on the
    /// next `resolve`/`len` call, since definitions are compiled on demand.
    pub fn set_placeholder_prefix(&self, prefix: char) {
        *self.placeholder_prefix.lock().unwrap() = prefix;
        self.version.fetch_add(1, Ordering::Relaxed);
    }

    pub fn remove_file(&self, uri: &Uri) {
        self.by_file.remove(uri);
        self.version.fetch_add(1, Ordering::Relaxed);
    }

    pub fn len(&self) -> usize {
        self.all_definitions().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns all definitions workspace-wide, rebuilding from the raw
    /// per-file defs only when a mutation happened since (version lag).
    /// The returned `Arc` is cheap to clone, so per-keystroke callers share
    /// one compiled snapshot instead of rebuilding (and re-cloning) it.
    fn all_definitions(&self) -> Arc<Vec<StepDefinition>> {
        let version = self.version.load(Ordering::Relaxed);
        let mut snapshot = self.snapshot.lock().unwrap();
        if snapshot.0 != version {
            let prefix = *self.placeholder_prefix.lock().unwrap();
            let mut defs = Vec::new();
            for entry in self.by_file.iter() {
                defs.extend(build_step_definitions(entry.key(), entry.value(), prefix));
            }
            *snapshot = (version, Arc::new(defs));
        }
        snapshot.1.clone()
    }

    pub fn resolve(&self, step_type: StepType, text: &str) -> ResolvedMatch {
        let defs = self.all_definitions();
        match resolve(&defs, step_type, text) {
            MatchOutcome::None => ResolvedMatch::None,
            MatchOutcome::Unique(d) => ResolvedMatch::Unique(d.clone()),
            MatchOutcome::Ambiguous {
                candidates,
                best_guess,
            } => ResolvedMatch::Ambiguous {
                candidates: candidates.into_iter().cloned().collect(),
                best_guess: best_guess.cloned(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(name: &str) -> Uri {
        format!("file:///{name}").parse::<Uri>().unwrap()
    }

    #[test]
    fn resolves_a_step_defined_in_one_file() {
        let index = StepDefinitionIndex::new();
        index.update_java_file(
            url("AccountSteps.java"),
            r#"
                public class AccountSteps {
                    @Given("$customer has $amount in account")
                    public void hasBalance(String customer, int amount) {}
                }
            "#,
        );
        match index.resolve(StepType::Given, "Bob has 100 in account") {
            ResolvedMatch::Unique(d) => assert_eq!(d.method_name, "hasBalance"),
            other => panic!("expected Unique, got {other:?}"),
        }
    }

    #[test]
    fn alias_inherits_step_type_from_sibling_annotation() {
        let index = StepDefinitionIndex::new();
        index.update_java_file(
            url("AccountSteps.java"),
            r#"
                public class AccountSteps {
                    @When("$customer withdraws $amount")
                    @Alias("$customer takes out $amount")
                    public void withdraw(String customer, int amount) {}
                }
            "#,
        );
        match index.resolve(StepType::When, "Bob takes out 50") {
            ResolvedMatch::Unique(d) => {
                assert_eq!(d.method_name, "withdraw");
                assert!(d.is_alias);
            }
            other => panic!("expected Unique, got {other:?}"),
        }
        // The canonical @When pattern still resolves too, and both point at
        // the same method so neither is reported as ambiguous with the other.
        match index.resolve(StepType::When, "Bob withdraws 50") {
            ResolvedMatch::Unique(d) => assert!(!d.is_alias),
            other => panic!("expected Unique, got {other:?}"),
        }
    }

    #[test]
    fn updating_a_file_replaces_its_previous_definitions() {
        let index = StepDefinitionIndex::new();
        let file = url("AccountSteps.java");
        index.update_java_file(
            file.clone(),
            r#"public class A { @Given("old pattern") public void a() {} }"#,
        );
        assert!(matches!(
            index.resolve(StepType::Given, "old pattern"),
            ResolvedMatch::Unique(_)
        ));

        index.update_java_file(
            file,
            r#"public class A { @Given("new pattern") public void a() {} }"#,
        );
        assert!(matches!(
            index.resolve(StepType::Given, "old pattern"),
            ResolvedMatch::None
        ));
        assert!(matches!(
            index.resolve(StepType::Given, "new pattern"),
            ResolvedMatch::Unique(_)
        ));
    }

    #[test]
    fn removing_a_file_drops_its_definitions() {
        let index = StepDefinitionIndex::new();
        let file = url("AccountSteps.java");
        index.update_java_file(
            file.clone(),
            r#"public class A { @Given("some pattern") public void a() {} }"#,
        );
        assert_eq!(index.len(), 1);
        index.remove_file(&file);
        assert!(index.is_empty());
        assert!(matches!(
            index.resolve(StepType::Given, "some pattern"),
            ResolvedMatch::None
        ));
    }

    #[test]
    fn ambiguous_across_two_files() {
        let index = StepDefinitionIndex::new();
        index.update_java_file(
            url("A.java"),
            r#"public class A { @Given("$x has $y") public void vague() {} }"#,
        );
        index.update_java_file(
            url("B.java"),
            r#"public class B { @Given("$x has 100") public void specific() {} }"#,
        );
        match index.resolve(StepType::Given, "Bob has 100") {
            ResolvedMatch::Ambiguous {
                candidates,
                best_guess,
            } => {
                assert_eq!(candidates.len(), 2);
                assert_eq!(best_guess.unwrap().method_name, "specific");
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn method_ranges_use_utf16_columns() {
        // `è` is 2 bytes but 1 UTF-16 unit, so the `@Given` starting the
        // method sits at byte column 22 but UTF-16 column 21.
        let index = StepDefinitionIndex::new();
        index.update_java_file(
            url("Regle.java"),
            "public class Règle { @Given(\"x\") public void a() {} }",
        );
        match index.resolve(StepType::Given, "x") {
            ResolvedMatch::Unique(d) => {
                assert_eq!(d.method_range.start.line, 0);
                assert_eq!(d.method_range.start.character, 21);
            }
            other => panic!("expected Unique, got {other:?}"),
        }
    }

    #[test]
    fn custom_placeholder_prefix_is_used_for_resolution_and_specificity() {
        let index = StepDefinitionIndex::new();
        index.set_placeholder_prefix('%');
        index.update_java_file(
            url("A.java"),
            r#"public class A { @Given("%user has %amount") public void vague() {} }"#,
        );
        index.update_java_file(
            url("B.java"),
            r#"public class B { @Given("%user has 100") public void specific() {} }"#,
        );

        // `%user` is treated as a placeholder, so both patterns match the
        // input and ambiguity resolution still prefers the literal one.
        match index.resolve(StepType::Given, "Bob has 100") {
            ResolvedMatch::Ambiguous {
                candidates,
                best_guess,
            } => {
                assert_eq!(candidates.len(), 2);
                assert_eq!(best_guess.unwrap().method_name, "specific");
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }

        // Reverting to the default `$` prefix makes `%user` literal text, so
        // the input no longer matches either definition.
        index.set_placeholder_prefix(DEFAULT_PLACEHOLDER_PREFIX);
        assert!(matches!(
            index.resolve(StepType::Given, "Bob has 100"),
            ResolvedMatch::None
        ));
    }
}
