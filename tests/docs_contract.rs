//! Consistency checks that the compiler and the other test suites cannot make.
//!
//! `cargo` already proves the code compiles, the schema payload is well formed, and every
//! documented behavior has a behavioral test. What it cannot prove is that the prose stays
//! attached to reality: that documentation links resolve, and that every command the binary
//! actually dispatches is still tracked in the capability matrix. Those two properties used
//! to be enforced by an external Python script; keeping them here means the complete Cargo gate
//! has no second toolchain.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn markdown_files(directory: &Path, found: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(directory).expect("readable documentation directory");
    for entry in entries {
        let path = entry.expect("readable directory entry").path();
        if path.is_dir() {
            markdown_files(&path, found);
        } else if path.extension().is_some_and(|value| value == "md") {
            found.push(path);
        }
    }
}

fn all_markdown() -> Vec<PathBuf> {
    let root = repo_root();
    let mut found = vec![root.join("README.md"), root.join("THIRD_PARTY_NOTICES.md")];
    markdown_files(&root.join("docs"), &mut found);
    found.sort();
    found
}

/// Extracts the target of every inline markdown link, skipping external URLs and anchors.
fn local_link_targets(text: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while let Some(open) = text[index..].find("](") {
        let start = index + open + 2;
        let Some(close_offset) = text[start..].find(')') else {
            break;
        };
        let target = &text[start..start + close_offset];
        index = start + close_offset;
        let _ = bytes;

        let target = target.split_whitespace().next().unwrap_or(target);
        if target.is_empty()
            || target.starts_with('#')
            || target.starts_with("http://")
            || target.starts_with("https://")
            || target.starts_with("mailto:")
        {
            continue;
        }
        targets.push(target.split('#').next().unwrap_or(target).to_string());
    }
    targets
}

#[test]
fn every_local_documentation_link_resolves() {
    let root = repo_root();
    let mut broken = Vec::new();

    for document in all_markdown() {
        let text = fs::read_to_string(&document).expect("readable markdown document");
        let parent = document.parent().expect("document has a parent directory");
        for target in local_link_targets(&text) {
            let resolved = parent.join(&target);
            if !resolved.exists() {
                let relative = document.strip_prefix(&root).unwrap_or(&document);
                broken.push(format!("{} -> {target}", relative.display()));
            }
        }
    }

    assert!(
        broken.is_empty(),
        "documentation links must resolve to existing paths:\n  {}",
        broken.join("\n  ")
    );
}

#[test]
fn readme_documents_the_complete_verification_gate() {
    let readme = fs::read_to_string(repo_root().join("README.md")).expect("readable README");

    assert!(
        !readme.contains("`cargo test` is the complete verification gate"),
        "README must not repeat the stale claim: `cargo test` is the complete verification gate"
    );

    for command in [
        "cargo fmt --all -- --check",
        "cargo clippy --locked --workspace --all-targets --all-features -- -D warnings",
        "cargo test --locked --workspace --all-targets --all-features",
    ] {
        assert!(
            readme.contains(command),
            "README must list the complete verification gate command: {command}"
        );
    }
}

/// The set of commands the binary actually dispatches, read from its `match` arms.
fn dispatched_commands() -> BTreeSet<String> {
    let source = fs::read_to_string(repo_root().join("src/bin/perfectpixel.rs"))
        .expect("readable CLI source");
    let dispatch_start = source
        .find("fn run(")
        .expect("CLI source defines the dispatch function");
    let dispatch = &source[dispatch_start..];
    let dispatch_end = dispatch
        .find("unknown command")
        .expect("dispatch ends with the unknown-command arm");

    let mut commands = BTreeSet::new();
    for line in dispatch[..dispatch_end].lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix('"') else {
            continue;
        };
        let Some(name) = rest.split('"').next() else {
            continue;
        };
        if line.contains("=>") && !name.starts_with('-') && !name.is_empty() {
            commands.insert(name.to_string());
        }
    }
    assert!(
        !commands.is_empty(),
        "failed to parse any dispatched command from the CLI source"
    );
    commands
}

#[test]
fn schema_output_lists_exactly_the_dispatched_commands() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_perfectpixel"))
        .arg("schema")
        .output()
        .expect("run perfectpixel schema");
    assert!(output.status.success(), "schema command must succeed");
    let payload: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("schema emits JSON");
    let advertised = payload["commands"]
        .as_array()
        .expect("schema payload lists commands")
        .iter()
        .map(|value| {
            value
                .as_str()
                .expect("command entries are strings")
                .to_string()
        })
        .collect::<BTreeSet<_>>();

    assert_eq!(
        advertised,
        dispatched_commands(),
        "`perfectpixel schema` must advertise exactly the commands the binary dispatches"
    );
}

#[test]
fn capability_matrix_traces_every_public_command() {
    let matrix = fs::read_to_string(repo_root().join("docs/FUNCTION_MATRIX.md"))
        .expect("readable capability matrix");
    let untracked = dispatched_commands()
        .into_iter()
        // `schema` reports the surface rather than being a capability of its own.
        .filter(|command| command != "schema")
        .filter(|command| !matrix.contains(command.as_str()))
        .collect::<Vec<_>>();

    assert!(
        untracked.is_empty(),
        "docs/FUNCTION_MATRIX.md must trace every public command; missing: {}",
        untracked.join(", ")
    );
}

#[test]
fn no_generated_or_platform_metadata_files_are_committed() {
    fn scan(directory: &Path, offenders: &mut Vec<String>, root: &Path) {
        for entry in fs::read_dir(directory).expect("readable directory") {
            let path = entry.expect("readable directory entry").path();
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_string();
            if name == "target" || name == ".git" {
                continue;
            }
            if path.is_dir() {
                scan(&path, offenders, root);
            } else if name == ".DS_Store" || name.ends_with(".orig") || name.ends_with(".rej") {
                let relative = path.strip_prefix(root).unwrap_or(&path);
                offenders.push(relative.display().to_string());
            }
        }
    }

    let root = repo_root();
    let mut offenders = Vec::new();
    scan(&root, &mut offenders, &root);
    assert!(
        offenders.is_empty(),
        "generated or platform metadata files must not be committed:\n  {}",
        offenders.join("\n  ")
    );
}
