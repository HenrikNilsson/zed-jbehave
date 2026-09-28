mod common;
pub mod groovy_scanner;
pub mod scanner;

/// A step-definition method found by scanning a `.java`/`.groovy` source
/// file, before it's been turned into a `matcher::StepDefinition` (which
/// needs a compiled regex and an LSP `Uri`/`Range` that the caller attaches
/// once it knows which file this came from).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawStepDefinition {
    /// The annotation this pattern came from: "Given", "When", "Then",
    /// "Alias", or "Aliases". Callers resolve `Alias`/`Aliases` patterns to
    /// the step type of the method's own `Given`/`When`/`Then` annotation.
    pub annotation: String,
    pub patterns: Vec<String>,
    pub declaring_class: String,
    pub method_name: String,
    /// 0-based (row, column) with the column in LSP (UTF-16) units —
    /// converted at scan time (see `common.rs`), since the index only ever
    /// forwards these to the client.
    pub method_start: (usize, usize),
    pub method_end: (usize, usize),
}
