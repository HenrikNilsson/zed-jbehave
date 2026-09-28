use tower_lsp_server::ls_types::Position;

/// Converts a tree-sitter `Point` (0-based row, **byte** column) into an LSP
/// `Position` (0-based line, **UTF-16** column) using the source text the
/// point came from.
///
/// tree-sitter counts columns in bytes while LSP counts them in UTF-16 code
/// units, so for any line containing non-ASCII text the two disagree (e.g.
/// `è` is 2 bytes but 1 unit, `🎉` is 4 bytes but 2 units). Copying the byte
/// column straight into `Position.character` misplaces every range on such
/// lines — and this grammar explicitly supports CJK/accented keywords, so
/// non-ASCII step lines are expected, not edge cases.
pub fn to_lsp_position(source: &str, row: usize, byte_col: usize) -> Position {
    let line = source.lines().nth(row).unwrap_or("");
    // Count UTF-16 units for whole chars starting before `byte_col`;
    // `take_while` (rather than slicing) stays correct even if a caller ever
    // passes a mid-character boundary.
    let units: usize = line
        .char_indices()
        .take_while(|(i, _)| *i < byte_col)
        .map(|(_, c)| c.len_utf16())
        .sum();
    Position::new(row as u32, units as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_columns_pass_through_unchanged() {
        assert_eq!(to_lsp_position("Given abc", 0, 9), Position::new(0, 9));
    }

    #[test]
    fn bmp_chars_count_as_one_unit_despite_multibyte_utf8() {
        // "Given règle ✓ done": è = 2 bytes/1 unit, ✓ = 3 bytes/1 unit.
        let line = "Given règle ✓ done";
        assert_eq!(line.len(), 21); // bytes
        assert_eq!(to_lsp_position(line, 0, 21), Position::new(0, 18));
    }

    #[test]
    fn astral_chars_count_as_two_units() {
        // "🎉" = 4 bytes, 2 UTF-16 units.
        assert_eq!(to_lsp_position("🎉 x", 0, 4), Position::new(0, 2));
        assert_eq!(to_lsp_position("🎉 x", 0, 6), Position::new(0, 4));
    }

    #[test]
    fn out_of_range_rows_and_columns_degrade_gracefully() {
        assert_eq!(to_lsp_position("ab", 5, 1), Position::new(5, 0));
        assert_eq!(to_lsp_position("ab", 0, 99), Position::new(0, 2));
    }
}
