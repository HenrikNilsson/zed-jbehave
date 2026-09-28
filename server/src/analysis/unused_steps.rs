use crate::matcher::{MatchOutcome, StepDefinition, StepType, resolve};

/// A step usage found in a `.story` file: its type and literal text (with
/// the keyword stripped). Once `story::mod` can parse `.story` buffers,
/// these come from every step across every story in the workspace; for now
/// this module is tested against synthetic usage lists.
#[derive(Debug, Clone)]
pub struct StepUsage {
    pub step_type: StepType,
    pub text: String,
}

/// Returns every `StepDefinition` that no `StepUsage` in `usages` resolves
/// to -- IntelliJ's "unused step declaration" inspection.
///
/// A definition counts as used if it, or any other definition (e.g. an
/// alias) sharing its declaring class and method name, is among the
/// resolved matches for at least one usage. Ambiguous matches count towards
/// *all* candidates rather than none: an ambiguous story step still
/// exercises each of those methods at runtime (JBehave picks one, but which
/// one can depend on registration order), and flagging a genuinely-called
/// method as "unused" would be a worse false positive than under-flagging
/// an ambiguous one.
pub fn find_unused<'a>(
    defs: &'a [StepDefinition],
    usages: &[StepUsage],
) -> Vec<&'a StepDefinition> {
    let mut used_methods: Vec<(&str, &str)> = Vec::new();
    for usage in usages {
        match resolve(defs, usage.step_type, &usage.text) {
            MatchOutcome::None => {}
            MatchOutcome::Unique(d) => used_methods.push((&d.declaring_class, &d.method_name)),
            MatchOutcome::Ambiguous { candidates, .. } => {
                used_methods.extend(
                    candidates
                        .iter()
                        .map(|c| (c.declaring_class.as_str(), c.method_name.as_str())),
                );
            }
        }
    }

    defs.iter()
        .filter(|d| {
            !used_methods
                .iter()
                .any(|(class, method)| *class == d.declaring_class && *method == d.method_name)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matcher::{DEFAULT_PLACEHOLDER_PREFIX, compile_pattern};
    use tower_lsp_server::ls_types::{Range, Uri};

    fn def(pattern: &str, class: &str, method: &str, alias: bool) -> StepDefinition {
        StepDefinition {
            step_type: StepType::Given,
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

    fn usage(text: &str) -> StepUsage {
        StepUsage {
            step_type: StepType::Given,
            text: text.to_string(),
        }
    }

    #[test]
    fn flags_a_never_referenced_step() {
        let defs = vec![def("a lonely step", "Steps", "lonely", false)];
        let unused = find_unused(&defs, &[]);
        assert_eq!(unused.len(), 1);
        assert_eq!(unused[0].method_name, "lonely");
    }

    #[test]
    fn does_not_flag_a_referenced_step() {
        let defs = vec![def("a used step", "Steps", "used", false)];
        let unused = find_unused(&defs, &[usage("a used step")]);
        assert!(unused.is_empty());
    }

    #[test]
    fn a_used_alias_marks_the_whole_method_as_used() {
        let defs = vec![
            def("canonical text", "Steps", "shared", false),
            def("alias text", "Steps", "shared", true),
        ];
        // Only the alias pattern is ever used in a story...
        let unused = find_unused(&defs, &[usage("alias text")]);
        // ...but since both entries share the same declaring method, neither
        // is reported unused.
        assert!(unused.is_empty());
    }

    #[test]
    fn ambiguous_usage_marks_all_candidates_as_used() {
        let defs = vec![
            def("$x has $y", "A", "vague", false),
            def("$x has 100", "B", "specific", false),
        ];
        let unused = find_unused(&defs, &[usage("Bob has 100")]);
        assert!(unused.is_empty());
    }

    #[test]
    fn unrelated_definitions_are_still_flagged_alongside_used_ones() {
        let defs = vec![
            def("used step", "Steps", "used", false),
            def("unused step", "Steps", "unused", false),
        ];
        let unused = find_unused(&defs, &[usage("used step")]);
        assert_eq!(unused.len(), 1);
        assert_eq!(unused[0].method_name, "unused");
    }
}
