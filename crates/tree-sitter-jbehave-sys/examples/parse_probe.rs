//! Probe the vendored grammar against real-world stories. Not part of the test
//! suite; run explicitly to hunt OOM triggers (see docs/OOM-INVESTIGATION.md).
//! Usage: cargo run -p tree-sitter-jbehave-sys --example parse_probe -- /path/to/stories
use std::path::{Path, PathBuf};
use std::time::Instant;

fn parse_one(parser: &mut tree_sitter::Parser, path: &Path) -> (usize, u128, usize) {
    let src = std::fs::read_to_string(path).expect("read");
    let start = Instant::now();
    let tree = parser.parse(&src, None).expect("parse");
    let elapsed = start.elapsed().as_millis();
    let nodes = count_nodes(tree.root_node());
    (src.len(), elapsed, nodes)
}

fn count_nodes(node: tree_sitter::Node) -> usize {
    let mut n = 1usize;
    let mut c = node.walk();
    if c.goto_first_child() {
        loop {
            n += count_nodes(c.node());
            if !c.goto_next_sibling() {
                break;
            }
        }
    }
    n
}

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("pass a stories root dir");
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_jbehave_sys::LANGUAGE.into())
        .expect("load grammar");

    let mut files: Vec<PathBuf> = Vec::new();
    let mut stack = vec![PathBuf::from(&root)];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().map(|s| s == "story").unwrap_or(false) {
                files.push(p);
            }
        }
    }
    files.sort();
    println!("parsing {} stories", files.len());

    let deadline = Instant::now() + std::time::Duration::from_secs(30);
    let mut worst: Vec<(u128, PathBuf)> = Vec::new();
    for f in &files {
        if Instant::now() > deadline {
            println!(
                "[timeout after 30s, {} of {} done]",
                worst.len() + files.len() - files.len(),
                files.len()
            );
        }
        let (bytes, ms, nodes) = parse_one(&mut parser, f);
        worst.push((ms, f.clone()));
        if ms > 500 || nodes > 100_000 {
            println!("{ms:>5}ms  {bytes:>7}B  {nodes:>8} nodes  {}", f.display());
        }
    }
    worst.sort();
    println!("--- slowest 5 ---");
    for (ms, f) in worst.iter().rev().take(5) {
        println!("{ms:>5}ms  {}", f.display());
    }
}
