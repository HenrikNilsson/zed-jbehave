fn main() {
    let src_dir = std::path::Path::new("vendor/src");

    let mut c_config = cc::Build::new();
    c_config.std("c11").include(src_dir);
    // The vendored sources are byte-identical to the sibling grammar repo's,
    // whose scanner.c carries two benign warnings; silence just those so
    // `make check`/`make clippy` stay clean without forking the vendored C —
    // and without a blanket `warnings(false)` that would also hide real
    // problems introduced by a future re-vendor.
    c_config
        .flag("-Wno-unused-function")
        .flag("-Wno-sign-compare");

    #[cfg(target_env = "msvc")]
    c_config.flag("-utf-8");

    let parser_path = src_dir.join("parser.c");
    c_config.file(&parser_path);
    println!("cargo:rerun-if-changed={}", parser_path.to_str().unwrap());

    let scanner_path = src_dir.join("scanner.c");
    if scanner_path.exists() {
        c_config.file(&scanner_path);
        println!("cargo:rerun-if-changed={}", scanner_path.to_str().unwrap());
    }

    c_config.compile("tree-sitter-jbehave");
}
