// ===========================================================================
// Integration Tests - Run Command (headless snap)
// ===========================================================================

mod common;

use std::path::PathBuf;
use std::process::Command;

use common::*;

/// Run `wt run --json` and return (exit_code, parsed JSON).
fn wt_run_json(repo: &PathBuf, home: &PathBuf, args: &[&str]) -> (Option<i32>, serde_json::Value) {
    let output = Command::new(wt_binary())
        .arg("run")
        .arg("--json")
        .args(args)
        .current_dir(repo)
        .env("HOME", home)
        .output()
        .expect("wt run failed to execute");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let doc: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "stdout must be valid JSON: {e}\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code(), doc)
}

#[test]
fn test_run_no_changes_cleans_up() {
    let (_dir, repo, home) = setup_worktree_test_env();

    let (code, doc) = wt_run_json(&repo, &home, &["run-clean", "--", "true"]);

    assert_eq!(code, Some(0));
    assert_eq!(doc["version"], 1);
    assert_eq!(doc["branch"], "run-clean");
    assert_eq!(doc["agent_exit_code"], 0);
    assert_eq!(doc["outcome"], "no_changes");
    assert_eq!(doc["worktree_removed"], true);
    assert_eq!(doc["exit_code"], 0);
    assert!(!PathBuf::from(doc["path"].as_str().unwrap()).exists());
}

#[test]
fn test_run_agent_failure_preserves_worktree() {
    let (_dir, repo, home) = setup_worktree_test_env();

    let (code, doc) = wt_run_json(&repo, &home, &["run-fail", "--", "false"]);

    assert_eq!(code, Some(10));
    assert_eq!(doc["outcome"], "agent_failed");
    assert_eq!(doc["agent_exit_code"], 1);
    assert_eq!(doc["worktree_removed"], false);
    assert!(PathBuf::from(doc["path"].as_str().unwrap()).exists());
}

#[test]
fn test_run_uncommitted_preserves_worktree() {
    let (_dir, repo, home) = setup_worktree_test_env();

    let (code, doc) = wt_run_json(
        &repo,
        &home,
        &["run-dirty", "--", "sh", "-c", "echo scratch > scratch.txt"],
    );

    assert_eq!(code, Some(11));
    assert_eq!(doc["outcome"], "uncommitted");
    assert!(doc["uncommitted"].as_u64().unwrap() > 0);
    assert_eq!(doc["worktree_removed"], false);
    assert!(PathBuf::from(doc["path"].as_str().unwrap()).exists());
}

#[test]
fn test_run_commits_default_keeps_worktree() {
    let (_dir, repo, home) = setup_worktree_test_env();

    let (code, doc) = wt_run_json(
        &repo,
        &home,
        &[
            "run-keep",
            "--",
            "sh",
            "-c",
            "echo done > done.txt && git add -A && git commit -q -m agent-work",
        ],
    );

    assert_eq!(code, Some(0));
    assert_eq!(doc["outcome"], "kept");
    assert_eq!(doc["commits"], 1);
    assert_eq!(doc["worktree_removed"], false);
    assert!(PathBuf::from(doc["path"].as_str().unwrap()).exists());
    // Not merged: the file must not appear in the main repo.
    assert!(!repo.join("done.txt").exists());
}

#[test]
fn test_run_on_success_merge_merges_and_cleans_up() {
    let (_dir, repo, home) = setup_worktree_test_env();

    let (code, doc) = wt_run_json(
        &repo,
        &home,
        &[
            "run-merge",
            "--on-success",
            "merge",
            "--",
            "sh",
            "-c",
            "echo merged > merged.txt && git add -A && git commit -q -m agent-work",
        ],
    );

    assert_eq!(code, Some(0));
    assert_eq!(doc["outcome"], "merged");
    assert_eq!(doc["merge_target"], "main");
    assert_eq!(doc["worktree_removed"], true);
    assert!(!PathBuf::from(doc["path"].as_str().unwrap()).exists());
    // Squash-merged into main: the file must appear in the main repo.
    assert!(repo.join("merged.txt").exists());
}

#[test]
fn test_run_merge_conflict_preserves_worktree() {
    let (_dir, repo, home) = setup_worktree_test_env();

    // The agent commits a README change in the worktree AND a conflicting
    // change on main (via the injected WT_MAIN_REPO), forcing a merge conflict.
    let (code, doc) = wt_run_json(
        &repo,
        &home,
        &[
            "run-conflict",
            "--on-success",
            "merge",
            "--",
            "sh",
            "-c",
            "echo worktree-side > README.md && git add -A && git commit -q -m wt-side && \
             cd \"$WT_MAIN_REPO\" && echo main-side > README.md && \
             git add -A && git commit -q -m main-side",
        ],
    );

    assert_eq!(code, Some(13));
    assert_eq!(doc["outcome"], "merge_conflict");
    assert_eq!(doc["worktree_removed"], false);
    assert!(PathBuf::from(doc["path"].as_str().unwrap()).exists());
    // Atomic semantics: main repo restored, no half-merged state.
    let readme = std::fs::read_to_string(repo.join("README.md")).unwrap();
    assert_eq!(readme.trim(), "main-side");
}

#[test]
fn test_run_requires_agent_command() {
    let (_dir, repo, home) = setup_worktree_test_env();

    let output = Command::new(wt_binary())
        .args(["run", "some-branch"])
        .current_dir(&repo)
        .env("HOME", &home)
        .output()
        .expect("wt run failed to execute");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("CMD") || stderr.contains("required"),
        "Expected missing-command error, got: {stderr}"
    );
}

#[test]
fn test_run_human_output_without_json() {
    let (_dir, repo, home) = setup_worktree_test_env();

    let output = Command::new(wt_binary())
        .args(["run", "run-human", "--", "true"])
        .current_dir(&repo)
        .env("HOME", &home)
        .output()
        .expect("wt run failed to execute");

    assert!(output.status.success());
    // Without --json, stdout carries no wt output (agent stdout passthrough only).
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No changes detected"),
        "Expected cleanup message, got: {stderr}"
    );
}
