//! Repo hygiene gate — compiled from CLAUDE.local.md by prompt-to-code.
//!
//! Enforces two declared project rules as a native, self-testing cargo test:
//!   - "files stay <= 500 lines"            (source: CLAUDE.local.md, section rules)
//!   - "comments and code text are English" (source: CLAUDE.local.md, section rules)
//!
//! The pure helpers below hold the logic and are unit-tested with pass/fail
//! fixtures; the repo-wide tests apply them to every .rs file under src/ and
//! tests/ and double as the drift detector — if the toolchain, layout, or a
//! future edit reintroduces a violation, `cargo test` fails loudly.
//!
//! This file must itself stay ASCII: `no_cjk_in_rust_source` scans tests/ too,
//! so the CJK fixtures below are built from code points at runtime rather than
//! written as literals (the same trick secret-scanner fixtures use).

use std::path::{Path, PathBuf};

const MAX_LINES: usize = 500;

/// True if any CJK code point appears in `text`. Ranges cover CJK symbols and
/// punctuation, Unified Ideographs (+ Extension A), compatibility ideographs,
/// and fullwidth/halfwidth forms — matching the intake scanner's ranges.
fn contains_cjk(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(
            c as u32,
            0x3000..=0x303F | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF
        )
    })
}

/// Line count, using the same notion of a line as the human-facing `wc -l`
/// comparison that surfaced the rule.
fn line_count(text: &str) -> usize {
    text.lines().count()
}

/// All `.rs` files under src/ and tests/ (target/ and npm/ are excluded by
/// only descending those two roots).
fn rust_sources() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in ["src", "tests"] {
        collect(Path::new(root), &mut out);
    }
    out
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_rust_file_exceeds_max_lines() {
    let offenders: Vec<String> = rust_sources()
        .into_iter()
        .filter_map(|path| {
            let count = line_count(&std::fs::read_to_string(&path).unwrap());
            (count > MAX_LINES).then(|| format!("{} ({count} lines)", path.display()))
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "Rule 'files stay <= {MAX_LINES} lines' violated — split these files:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn no_cjk_in_rust_source() {
    let offenders: Vec<String> = rust_sources()
        .into_iter()
        .flat_map(|path| {
            let text = std::fs::read_to_string(&path).unwrap();
            text.lines()
                .enumerate()
                .filter(|(_, line)| contains_cjk(line))
                .map(|(index, line)| format!("{}:{} {}", path.display(), index + 1, line.trim()))
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "Rule 'comments and code text are English' violated — translate to English:\n  {}",
        offenders.join("\n  ")
    );
}

// --- Helper unit tests: pass + fail fixtures, the drift detector proper. ---

#[test]
fn contains_cjk_flags_non_english_and_passes_ascii() {
    // Fail case: the exact shape this rule was created to stop — a Chinese
    // comment. Built from code points so this source file stays ASCII.
    let ideograph = char::from_u32(0x89E3).unwrap(); // CJK Unified Ideograph
    let fullwidth_comma = char::from_u32(0xFF0C).unwrap(); // fullwidth punctuation
    let corner_bracket = char::from_u32(0x300C).unwrap(); // CJK symbol/punctuation
    assert!(contains_cjk(&format!("// {ideograph} parse the version")));
    assert!(contains_cjk(&fullwidth_comma.to_string()));
    assert!(contains_cjk(&corner_bracket.to_string()));

    // Pass case: English-only source lines.
    assert!(!contains_cjk("// Parse JSON to read the version field"));
    assert!(!contains_cjk("let message = \"hello, world\";"));
    assert!(!contains_cjk("if count <= 500 { ok() }"));
}

#[test]
fn line_count_boundary_at_max() {
    let at_limit = "x\n".repeat(MAX_LINES);
    let over_limit = "x\n".repeat(MAX_LINES + 1);
    assert!(line_count(&at_limit) <= MAX_LINES); // pass case
    assert!(line_count(&over_limit) > MAX_LINES); // fail case
}
