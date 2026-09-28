use super::RawStepDefinition;
use crate::positions::to_lsp_position;
use tree_sitter::{Language, Node, Parser};

const STEP_ANNOTATIONS: [&str; 5] = ["Given", "When", "Then", "Alias", "Aliases"];

fn text<'a>(node: Node, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

/// Strips a string literal's surrounding quotes. Doesn't yet unescape
/// `\"`/`\\`/text-block (`"""`) content precisely — good enough for now
/// since JBehave step patterns are rarely more than plain ASCII text; revisit
/// if that turns out to matter in practice.
fn unquote(raw: &str) -> String {
    raw.trim_start_matches("\"\"\"")
        .trim_end_matches("\"\"\"")
        .trim_matches('"')
        .trim()
        .to_string()
}

fn collect_string_literals(node: Node, source: &str, out: &mut Vec<String>) {
    if node.kind() == "string_literal" {
        out.push(unquote(text(node, source)));
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_string_literals(child, source, out);
    }
}

fn lsp_point(source: &str, p: tree_sitter::Point) -> (usize, usize) {
    let pos = to_lsp_position(source, p.row, p.column);
    (pos.line as usize, pos.character as usize)
}

fn extract_from_method(node: Node, source: &str, declaring_class: &str) -> Vec<RawStepDefinition> {
    let mut defs = Vec::new();
    let Some(name_node) = node.child_by_field_name("name") else {
        return defs;
    };
    let method_name = text(name_node, source).to_string();

    let mut cursor = node.walk();
    let Some(modifiers) = node.children(&mut cursor).find(|c| c.kind() == "modifiers") else {
        return defs;
    };

    let mut mod_cursor = modifiers.walk();
    for child in modifiers.children(&mut mod_cursor) {
        if child.kind() != "annotation" && child.kind() != "marker_annotation" {
            continue;
        }
        let Some(ann_name_node) = child.child_by_field_name("name") else {
            continue;
        };
        let ann_name = text(ann_name_node, source).to_string();
        if !STEP_ANNOTATIONS.contains(&ann_name.as_str()) {
            continue;
        }
        let mut patterns = Vec::new();
        collect_string_literals(child, source, &mut patterns);
        if patterns.is_empty() {
            continue;
        }
        defs.push(RawStepDefinition {
            annotation: ann_name,
            patterns,
            declaring_class: declaring_class.to_string(),
            method_name: method_name.clone(),
            // Converted to LSP (UTF-16) columns here, while the source is
            // still at hand — the index only ever forwards these to the
            // client, never back into tree-sitter.
            method_start: lsp_point(source, node.start_position()),
            method_end: lsp_point(source, node.end_position()),
        });
    }
    defs
}

fn walk(node: Node, source: &str, current_class: &str, out: &mut Vec<RawStepDefinition>) {
    let mut class_name = current_class.to_string();
    match node.kind() {
        "class_declaration"
        | "interface_declaration"
        | "record_declaration"
        | "enum_declaration"
        | "trait_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                class_name = text(name_node, source).to_string();
            }
        }
        "method_declaration" => {
            out.extend(extract_from_method(node, source, &class_name));
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, source, &class_name, out);
    }
}

/// Shared scan implementation: both `scanner.rs` (Java) and
/// `groovy_scanner.rs` use it since `tree-sitter-groovy` mirrors
/// `tree-sitter-java`'s node kinds and field names closely enough (verified
/// by dumping a sample parse tree) that the same extraction logic applies to
/// both.
pub fn scan_with_language(source: &str, language: Language) -> Vec<RawStepDefinition> {
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .expect("loading the grammar should never fail");
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    walk(tree.root_node(), source, "", &mut out);
    out
}
