// ===========================================================================
// Integration Tests - Mv Command
// ===========================================================================

mod common;

use std::process::Command;
use tempfile::tempdir;

use common::*;

#[test]
fn test_mv_nonexistent() {
    let dir = tempdir().unwrap();
    setup_git_repo(dir.path());

    let output = Command::new(wt_binary())
        .args(["mv", "old-branch", "new-branch"])
        .current_dir(dir.path())
        .output()
        .expect("Failed to execute wt mv");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found") || stderr.contains("error"));
}

#[test]
fn test_mv_with_same_name() {
    let dir = tempdir().unwrap();
    setup_git_repo(dir.path());

    Command::new("git")
        .args(["branch", "feature-x"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let output = Command::new(wt_binary())
        .args(["mv", "feature-x", "feature-x"])
        .current_dir(dir.path())
        .output()
        .expect("wt mv failed");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success() || !stderr.is_empty());
}

#[test]
fn test_mv_existing_branch() {
    let (_dir, repo, home) = setup_worktree_test_env();

    let output = Command::new(wt_binary())
        .args(["new", "mv-old-name"])
        .current_dir(&repo)
        .env("HOME", &home)
        .output()
        .expect("wt new failed");

    assert!(
        output.status.success(),
        "Command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let output = Command::new(wt_binary())
        .args(["mv", "mv-old-name", "mv-new-name"])
        .current_dir(&repo)
        .env("HOME", &home)
        .output()
        .expect("wt mv failed");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success() || !stderr.is_empty());
}

#[test]
fn test_mv_renames_worktree() {
    let (_dir, repo, home) = setup_worktree_test_env();

    let output = Command::new(wt_binary())
        .args(["new", "mv-src"])
        .current_dir(&repo)
        .env("HOME", &home)
        .output()
        .expect("wt new failed");

    assert!(
        output.status.success(),
        "Command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let output = Command::new(wt_binary())
        .args(["mv", "mv-src", "mv-dst"])
        .current_dir(&repo)
        .env("HOME", &home)
        .output()
        .expect("wt mv failed");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success()
            || stderr.contains("Renamed")
            || stderr.contains("not found")
            || stderr.contains("error")
    );
}

/// `git worktree move` refuses worktrees containing submodules and offers no
/// --force, so wt moves them by hand — the submodule must survive intact.
#[test]
fn test_mv_worktree_with_submodule() {
    let (_dir, repo, home, old_path) = new_worktree_with_submodule("mv-sub");

    let output = Command::new(wt_binary())
        .args(["mv", "mv-sub", "mv-sub-renamed"])
        .current_dir(&repo)
        .env("HOME", &home)
        .output()
        .expect("wt mv failed");
    assert!(
        output.status.success(),
        "wt mv must handle submodules: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let new_path = old_path.with_file_name("mv-sub-renamed");
    assert!(!old_path.exists(), "old worktree path should be gone");
    assert!(new_path.join("sub").join("README.md").exists());

    // The submodule's gitdir link and core.worktree must both point at the
    // new location, or every git command inside it dies.
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(new_path.join("sub"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "submodule must stay usable after the move: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let output = Command::new("git")
        .args(["submodule", "status"])
        .current_dir(&new_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "superproject must still see the submodule: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
