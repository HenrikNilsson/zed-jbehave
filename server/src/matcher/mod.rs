use regex::Regex;
use tower_lsp_server::ls_types::{Range, Uri};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StepType {
    Given,
    When,
    Then,
}

impl StepType {
    pub fn as_str(&self) -> &'static str {
        match self {
            StepType::Given => "Given",
            StepType::When => "When",
            StepType::Then => "Then",
        }
    }
}

#[derive(Debug, Clone)]
pub struct StepDefinition {
    pub step_type: StepType,
    pub pattern_source: String,
    pub compiled: Regex,
    /// The parameter prefix (`$` by default, or a custom one like `%` or `!`)
    /// this pattern was compiled with. Kept on the definition so ambiguity
    /// resolution's specificity tiebreak uses the same prefix the pattern
    /// was actually compiled against.
    pub placeholder_prefix: char,
    pub declaring_class: String,
    pub method_name: String,
    pub file: Uri,
    pub method_range: Range,
    pub literal_range: Range,
    pub is_alias: bool,
}

fn same_method(a: &StepDefinition, b: &StepDefinition) -> bool {
    a.declaring_class == b.declaring_class && a.method_name == b.method_name
}

pub const DEFAULT_PLACEHOLDER_PREFIX: char = '$';

fn placeholder_re(prefix: char) -> Regex {
    // The prefix is user configuration (`parameter_prefix`), so escape it:
    // interpolated raw, a letter prefix either panics (`'p'` → invalid
    // `\p[…]` escape) or silently changes meaning (`'d'` → digit class).
    let mut prefix_src = String::new();
    prefix_src.push(prefix);
    Regex::new(&format!(
        r"{}[A-Za-z_][A-Za-z0-9_]*",
        regex::escape(&prefix_src)
    ))
    .unwrap()
}

/// Compiles a JBehave step pattern such as `"$customer has $amount in account"`
/// into an anchored, case-sensitive regex. Each `{prefix}identifier` placeholder
/// becomes a non-greedy capturing group, mirroring JBehave's own
/// `StepPatternParser` default behaviour closely enough for matching
/// purposes (this is for IDE-side resolution, not for running stories).
///
/// `prefix` is the JBehave parameter prefix (default `$`), configurable per
/// workspace for projects that use a non-standard one.
pub fn compile_pattern(pattern: &str, prefix: char) -> Result<Regex, regex::Error> {
    let mut regex_src = String::from("(?s)^");
    let mut last_end = 0;
    for m in placeholder_re(prefix).find_iter(pattern) {
        regex_src.push_str(&regex::escape(&pattern[last_end..m.start()]));
        regex_src.push_str("(.+?)");
        last_end = m.end();
    }
    regex_src.push_str(&regex::escape(&pattern[last_end..]));
    regex_src.push('$');
    Regex::new(&regex_src)
}

/// Number of non-placeholder characters in a pattern, used as a specificity
/// tiebreak: a pattern with more literal text and fewer/narrower wildcards
/// is considered the more specific match, mirroring how a human would expect
/// `"$x has 100"` to win over `"$x has $y"` for the input `"Bob has 100"`.
fn specificity(pattern: &str, prefix: char) -> usize {
    let mut literal_len = pattern.len();
    for m in placeholder_re(prefix).find_iter(pattern) {
        literal_len -= m.len();
    }
    literal_len
}

#[derive(Debug)]
pub enum MatchOutcome<'a> {
    None,
    Unique(&'a StepDefinition),
    /// More than one *distinct* step method matches. `best_guess` is set only
    /// when specificity strictly disambiguates the winner; it's still worth
    /// surfacing the full candidate list as a diagnostic even when a
    /// best_guess exists, since JBehave itself would throw
    /// `DuplicateCandidateFoundException` at runtime for a true tie.
    Ambiguous {
        candidates: Vec<&'a StepDefinition>,
        best_guess: Option<&'a StepDefinition>,
    },
}

pub fn resolve<'a>(
    defs: &'a [StepDefinition],
    step_type: StepType,
    text: &str,
) -> MatchOutcome<'a> {
    let candidates: Vec<&StepDefinition> = defs
        .iter()
        .filter(|d| d.step_type == step_type && d.compiled.is_match(text))
        .collect();

    let mut distinct: Vec<&StepDefinition> = Vec::new();
    for c in candidates {
        if !distinct.iter().any(|d| same_method(d, c)) {
            distinct.push(c);
        }
    }

    match distinct.len() {
        0 => MatchOutcome::None,
        1 => MatchOutcome::Unique(distinct[0]),
        _ => {
            let max_specificity = distinct
                .iter()
                .map(|d| specificity(&d.pattern_source, d.placeholder_prefix))
                .max()
                .unwrap();
            let winners: Vec<&StepDefinition> = distinct
                .iter()
                .filter(|d| specificity(&d.pattern_source, d.placeholder_prefix) == max_specificity)
                .copied()
                .collect();
            let best_guess = if winners.len() == 1 {
                Some(winners[0])
            } else {
                None
            };
            MatchOutcome::Ambiguous {
                candidates: distinct,
                best_guess,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(
        step_type: StepType,
        pattern: &str,
        class: &str,
        method: &str,
        alias: bool,
    ) -> StepDefinition {
        StepDefinition {
            step_type,
            pattern_source: pattern.to_string(),
            compiled: compile_pattern(pattern, DEFAULT_PLACEHOLDER_PREFIX).unwrap(),
            placeholder_prefix: DEFAULT_PLACEHOLDER_PREFIX,
            declaring_class: class.to_string(),
            method_name: method.to_string(),
            file: "file:///Steps.java".parse::<Uri>().unwrap(),
            method_range: Range::default(),
            literal_range: Range::default(),
            is_alias: alias,
        }
    }

    #[test]
    fn compiles_and_matches_simple_pattern() {
        let re = compile_pattern("$customer has $amount in account", '$').unwrap();
        let caps = re.captures("Bob has 100 in account").unwrap();
        assert_eq!(&caps[1], "Bob");
        assert_eq!(&caps[2], "100");
    }

    #[test]
    fn pattern_without_placeholders_matches_literally() {
        let re = compile_pattern("the system is idle", '$').unwrap();
        assert!(re.is_match("the system is idle"));
        assert!(!re.is_match("the system is idle now"));
    }

    #[test]
    fn resolve_returns_unique_match() {
        let defs = vec![def(
            StepType::Given,
            "$customer has $amount in account",
            "AccountSteps",
            "hasBalance",
            false,
        )];
        match resolve(&defs, StepType::Given, "Bob has 100 in account") {
            MatchOutcome::Unique(d) => assert_eq!(d.method_name, "hasBalance"),
            other => panic!("expected Unique, got {other:?}"),
        }
    }

    #[test]
    fn resolve_returns_none_when_nothing_matches() {
        let defs = vec![def(
            StepType::Given,
            "$customer has $amount in account",
            "AccountSteps",
            "hasBalance",
            false,
        )];
        assert!(matches!(
            resolve(&defs, StepType::When, "Bob has 100 in account"),
            MatchOutcome::None
        ));
        assert!(matches!(
            resolve(&defs, StepType::Given, "totally unrelated text"),
            MatchOutcome::None
        ));
    }

    #[test]
    fn aliases_of_the_same_method_are_not_ambiguous() {
        let defs = vec![
            def(
                StepType::Given,
                "$customer has $amount in account",
                "AccountSteps",
                "hasBalance",
                false,
            ),
            def(
                StepType::Given,
                "$customer's balance is $amount",
                "AccountSteps",
                "hasBalance",
                true,
            ),
        ];
        match resolve(&defs, StepType::Given, "Bob has 100 in account") {
            MatchOutcome::Unique(d) => assert_eq!(d.method_name, "hasBalance"),
            other => panic!("expected Unique (alias, same method), got {other:?}"),
        }
    }

    #[test]
    fn distinct_methods_matching_the_same_text_are_ambiguous_with_best_guess() {
        let defs = vec![
            def(StepType::Given, "$x has $y", "A", "vague", false),
            def(StepType::Given, "$x has 100", "B", "specific", false),
        ];
        match resolve(&defs, StepType::Given, "Bob has 100") {
            MatchOutcome::Ambiguous {
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
    fn true_tie_has_no_best_guess() {
        let defs = vec![
            def(StepType::Given, "$x has $y in account", "A", "one", false),
            def(StepType::Given, "$a has $b in account", "B", "two", false),
        ];
        match resolve(&defs, StepType::Given, "Bob has 100 in account") {
            MatchOutcome::Ambiguous {
                candidates,
                best_guess,
            } => {
                assert_eq!(candidates.len(), 2);
                assert!(best_guess.is_none());
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn custom_prefix_compiles_and_matches() {
        let re = compile_pattern("%customer has %amount in account", '%').unwrap();
        assert!(re.is_match("Bob has 100 in account"));
        // The same pattern under the default `$` prefix must NOT match, since
        // `%customer` is literal text when the prefix is `$`.
        assert!(
            !compile_pattern("%customer has %amount in account", '$')
                .unwrap()
                .is_match("Bob has 100 in account")
        );
    }

    #[test]
    fn custom_prefix_specificity_only_counts_its_own_placeholders() {
        let re = compile_pattern("%x has 100", '%').unwrap();
        assert!(re.is_match("Bob has 100"));
        // With a `%` prefix, `%x` is a placeholder so `literal_len` excludes it.
        assert_eq!(
            specificity("%x has 100", '%'),
            "%x has 100".len() - "%x".len()
        );
        // The default `$` prefix: `$x` is a placeholder.
        assert!(specificity("$x has 100", '$') < "$x has 100".len());
    }

    #[test]
    fn hostile_prefixes_are_matched_literally_without_panicking() {
        // `'p'` makes `\p[…]` (invalid escape → Regex::new errors);
        // `'d'`/`'w'`/`'s'` make valid-but-wrong classes (`\d` etc.);
        // `'\\'` and `'|'` are regex metacharacters outright. A prefix is
        // user configuration, so all of these must compile and match the
        // pattern text literally instead of panicking or changing meaning.
        for prefix in ['p', 'd', 'w', 's', '\\', '|', '['] {
            let re = compile_pattern("v2x field", prefix)
                .unwrap_or_else(|e| panic!("prefix {prefix:?} failed to compile: {e}"));
            assert!(
                re.is_match("v2x field"),
                "prefix {prefix:?} should match its own literal text"
            );
            assert!(
                !re.is_match("v9x field"),
                "prefix {prefix:?} must not treat 'v2x' as placeholder+literal"
            );
        }
    }

    #[test]
    fn resolve_uses_each_definitions_own_prefix_for_specificity() {
        let mut a = def(StepType::Given, "%x has %y", "A", "vague", false);
        a.placeholder_prefix = '%';
        a.compiled = compile_pattern("%x has %y", '%').unwrap();

        let mut b = def(StepType::Given, "%x has 100", "B", "specific", false);
        b.placeholder_prefix = '%';
        b.compiled = compile_pattern("%x has 100", '%').unwrap();

        match resolve(&[a, b], StepType::Given, "Bob has 100") {
            MatchOutcome::Ambiguous {
                candidates,
                best_guess,
            } => {
                assert_eq!(candidates.len(), 2);
                assert_eq!(best_guess.unwrap().method_name, "specific");
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }
}
