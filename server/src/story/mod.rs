use tower_lsp_server::ls_types::{Position, Range};
use tree_sitter::{Node, Parser};

use crate::matcher::StepType;
use crate::positions::to_lsp_position;

/// A single step in a `.story` file, resolved from a `step` node in the
/// tree-sitter-jbehave parse tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryStep {
    /// `Given`/`When`/`Then`. An `And` step inherits its type from the most
    /// recent explicit keyword within the same scenario (see `parse`).
    pub step_type: StepType,
    /// The step's literal text with the keyword stripped, exactly as the
    /// matcher's compiled regex expects to match against it. Built by slicing
    /// the `step_text` node's byte range verbatim (so `$param` placeholders
    /// and `<name>`-style Examples-table placeholders survive as raw text),
    /// then appending any `step_continuation_line` text joined by `\n`.
    /// `table_row` children are a separate structured argument and are not
    /// part of this text.
    pub text: String,
    /// The step's span: from the start of its keyword to the end of its last
    /// text-bearing line (continuation lines included, trailing `_eol`s and
    /// table rows excluded). Used for hit-testing a cursor position.
    pub range: Range,
}

#[derive(Debug, Default)]
pub struct ParsedStory {
    pub steps: Vec<StoryStep>,
}

/// Parses a `.story` buffer into its steps.
///
/// Only `source_file` → `scenario` → `step` is walked. `and_kw` inherits the
/// prior explicit `Given`/`When`/`Then` within that scenario; the inheritance
/// chain resets at each scenario boundary, mirroring what `matcher::resolve`
/// already assumes about how steps are typed. (Lifecycle `Before:`/`After:`
/// sections also contain steps but are not collected yet — see PARITY.md.)
///
/// The parser is error-tolerant, so a malformed story still yields whatever
/// steps parsed cleanly — which is what the diagnostics pass wants.
pub fn parse(source: &str) -> ParsedStory {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_jbehave_sys::LANGUAGE.into())
        .expect("the vendored jbehave grammar should always load");
    let Some(tree) = parser.parse(source, None) else {
        return ParsedStory::default();
    };
    let root = tree.root_node();
    let mut steps = Vec::new();
    let mut root_cursor = root.walk();
    for node in root.children(&mut root_cursor) {
        if node.kind() != "scenario" {
            continue;
        }
        let mut current_step_type: Option<StepType> = None;
        let mut scenario_cursor = node.walk();
        for child in node.children(&mut scenario_cursor) {
            if child.kind() == "step"
                && let Some(step) = parse_step(source, child, &mut current_step_type)
            {
                steps.push(step);
            }
        }
    }
    ParsedStory { steps }
}

/// Returns the step whose range contains `position`, or `None`. The range's
/// end is treated as inclusive so a cursor sitting at the end of a step's
/// last line still resolves to that step.
pub fn step_at(story: &ParsedStory, position: Position) -> Option<&StoryStep> {
    story.steps.iter().find(|step| {
        let (sl, sc) = (step.range.start.line, step.range.start.character);
        let (el, ec) = (step.range.end.line, step.range.end.character);
        let (pl, pc) = (position.line, position.character);
        (sl, sc) <= (pl, pc) && (pl, pc) <= (el, ec)
    })
}

fn parse_step(source: &str, node: Node, current: &mut Option<StepType>) -> Option<StoryStep> {
    let step_type = step_type_of(node, current)?;
    let text = step_text(source, node)?;
    let range = step_range(source, node);
    Some(StoryStep {
        step_type,
        text,
        range,
    })
}

fn child_of_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|c| c.kind() == kind)
}

fn step_type_of(step: Node, current: &mut Option<StepType>) -> Option<StepType> {
    let keyword = child_of_kind(step, "step_keyword")?;
    let mut cursor = keyword.walk();
    let mut explicit: Option<StepType> = None;
    let mut is_and = false;
    for k in keyword.children(&mut cursor) {
        match k.kind() {
            "given_kw" => explicit = Some(StepType::Given),
            "when_kw" => explicit = Some(StepType::When),
            "then_kw" => explicit = Some(StepType::Then),
            "and_kw" => is_and = true,
            _ => {}
        }
    }
    match (explicit, is_and) {
        (Some(t), _) => {
            *current = Some(t);
            Some(t)
        }
        // `And` inherits the current explicit type (without advancing it); a
        // scenario-leading `And` with nothing to inherit from is skipped.
        (None, true) => *current,
        (None, false) => None,
    }
}

/// Builds the literal step text: the `step_text` slice verbatim (minus
/// trailing whitespace), plus each `step_continuation_line`'s
/// `continuation_text` joined by `\n`. `table_row` children are skipped.
fn step_text(source: &str, node: Node) -> Option<String> {
    let mut parts = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "step_text" => parts.push(&source[child.byte_range()]),
            "step_continuation_line" => {
                if let Some(cont) = child_of_kind(child, "continuation_text") {
                    parts.push(&source[cont.byte_range()]);
                }
            }
            _ => {}
        }
    }
    if parts.is_empty() {
        return None;
    }
    // The grammar's step_text_fragment/continuation_text tokens include any
    // trailing whitespace before the newline (real files have it), which the
    // matcher's anchored regex would otherwise reject as extra input.
    let mut out = String::new();
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(part.trim_end());
    }
    Some(out)
}

fn step_range(source: &str, node: Node) -> Range {
    let start = node.start_position();
    let mut end = node.start_position();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(child.kind(), "step_keyword" | "step_text") {
            end = child.end_position();
        } else if child.kind() == "step_continuation_line"
            && let Some(cont) = child_of_kind(child, "continuation_text")
        {
            end = cont.end_position();
        }
    }
    Range::new(
        to_lsp_position(source, start.row, start.column),
        to_lsp_position(source, end.row, end.column),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn story(source: &str) -> ParsedStory {
        parse(source)
    }

    fn pos(line: u32, character: u32) -> Position {
        Position::new(line, character)
    }

    #[test]
    fn parses_step_types_and_verbatim_text() {
        let s = story(
            "Scenario: has balance\n\
             Given Bob has $amount in account\n\
             When Bob withdraws $amount\n\
             Then balance is $amount\n",
        );
        let types: Vec<StepType> = s.steps.iter().map(|st| st.step_type).collect();
        assert_eq!(types, vec![StepType::Given, StepType::When, StepType::Then]);
        let texts: Vec<&str> = s.steps.iter().map(|st| st.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "Bob has $amount in account",
                "Bob withdraws $amount",
                "balance is $amount",
            ]
        );
    }

    #[test]
    fn and_steps_inherit_the_current_explicit_type() {
        let s = story(
            "Scenario: s\n\
             Given a precondition\n\
             And another precondition\n\
             When an action happens\n\
             And another action\n",
        );
        let types: Vec<StepType> = s.steps.iter().map(|st| st.step_type).collect();
        assert_eq!(
            types,
            vec![
                StepType::Given,
                StepType::Given,
                StepType::When,
                StepType::When,
            ]
        );
        assert_eq!(s.steps[1].text, "another precondition");
    }

    #[test]
    fn and_inheritance_resets_across_scenarios() {
        let s = story(
            "Scenario: one\n\
             Given first\n\
             And second\n\
             Scenario: two\n\
             Then a result\n\
             And another result\n",
        );
        let types: Vec<StepType> = s.steps.iter().map(|st| st.step_type).collect();
        assert_eq!(
            types,
            vec![
                StepType::Given,
                StepType::Given,
                StepType::Then,
                StepType::Then
            ]
        );
    }

    #[test]
    fn and_with_no_prior_explicit_step_is_skipped() {
        let s = story("Scenario: s\nAnd orphaned\nGiven real\n");
        assert_eq!(s.steps.len(), 1);
        assert_eq!(s.steps[0].text, "real");
    }

    #[test]
    fn trailing_whitespace_is_trimmed_from_step_text() {
        let s = story("Scenario: s\nGiven a precondition   \n");
        assert_eq!(s.steps.len(), 1);
        assert_eq!(s.steps[0].text, "a precondition");
    }

    #[test]
    fn degenerate_inputs_terminate_with_no_steps() {
        // Regression net for the Zed OOM: the grammar's external scanner
        // used to mint zero-width EOLs at EOF forever, so any of these
        // hung the parse (and Zed with it). They must all return promptly.
        for source in ["", "\n", "   ", "Scenario: foo", "Scenario: foo\n"] {
            assert!(story(source).steps.is_empty(), "hung on {source:?}");
        }
    }

    #[test]
    fn continuation_lines_are_joined_by_newline_but_table_rows_are_not() {
        let s = story(
            "Scenario: s\n\
             Given an order was placed\n\
             and with extra details on the next line\n\
             |Name|Quantity|\n\
             |Widget|2|\n\
             When something happens\n",
        );
        assert_eq!(s.steps.len(), 2);
        assert_eq!(
            s.steps[0].text,
            "an order was placed\nand with extra details on the next line"
        );
        assert_eq!(s.steps[1].text, "something happens");
    }

    #[test]
    fn narrative_prose_is_not_collected_as_steps() {
        let s = story(
            "Narrative:\n\
             Given the above context, proceed with caution.\n\
             When in doubt, ask.\n\
             \n\
             Scenario: real\n\
             Given a real step\n",
        );
        assert_eq!(s.steps.len(), 1);
        assert_eq!(s.steps[0].text, "a real step");
    }

    #[test]
    fn step_at_hits_keyword_middle_and_end_of_line() {
        let s = story("Scenario: s\nGiven Bob has $amount\nWhen it happens\n");
        // On the keyword itself.
        assert_eq!(step_at(&s, pos(1, 0)).unwrap().text, "Bob has $amount");
        // In the middle of the step text.
        assert_eq!(step_at(&s, pos(1, 10)).unwrap().text, "Bob has $amount");
        // At the very end of the step line (inclusive end).
        assert_eq!(step_at(&s, pos(1, 18)).unwrap().text, "Bob has $amount");
        // A different line resolves to that line's step.
        assert_eq!(step_at(&s, pos(2, 5)).unwrap().text, "it happens");
    }

    #[test]
    fn step_at_returns_none_outside_steps() {
        let s = story("Scenario: s\nGiven a step\n\nWhen another\n");
        // On the Scenario: header line.
        assert!(step_at(&s, pos(0, 4)).is_none());
        // On the blank line between steps.
        assert!(step_at(&s, pos(2, 0)).is_none());
        // Past the end of the last step's line (line 3, col 13 > line length
        // 12, which the inclusive-end check does not swallow).
        assert!(step_at(&s, pos(3, 13)).is_none());
    }

    #[test]
    fn step_at_hits_a_step_on_its_continuation_line() {
        let s = story(
            "Scenario: s\n\
             Given an order was placed\n\
             and a continuation here\n\
             When next\n",
        );
        assert_eq!(
            step_at(&s, pos(2, 7)).unwrap().text,
            "an order was placed\nand a continuation here"
        );
    }

    #[test]
    fn examples_table_placeholders_survive_as_raw_text() {
        let s = story("Scenario: s\nGiven user <name> exists\n");
        assert_eq!(s.steps[0].text, "user <name> exists");
    }

    #[test]
    fn step_ranges_are_exactly_the_text_lines() {
        let s = story("Scenario: s\nGiven first\nAnd second\n");
        assert_eq!(s.steps[0].range.start, pos(1, 0));
        assert_eq!(s.steps[0].range.end, pos(1, 11));
        assert_eq!(s.steps[1].range.start, pos(2, 0));
        assert_eq!(s.steps[1].range.end, pos(2, 10));
    }

    #[test]
    fn non_ascii_columns_are_utf16_not_bytes() {
        // "Given règle ✓ done": è is 2 bytes/1 unit, ✓ is 3 bytes/1 unit,
        // so the step ends at UTF-16 column 18 (byte column 21).
        let s = story("Scenario: s\nGiven règle ✓ done\n");
        assert_eq!(s.steps.len(), 1);
        assert_eq!(s.steps[0].range.end, pos(1, 18));
        // A client cursor at the UTF-16 end of line hits the step…
        assert_eq!(step_at(&s, pos(1, 18)).unwrap().text, "règle ✓ done");
        // …while byte-offset positions past it do not.
        assert!(step_at(&s, pos(1, 19)).is_none());
    }
}
