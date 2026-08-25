// ===========================================================================
// tests/common - Shared Test Helpers
// ===========================================================================

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

pub fn wt_binary() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("target/debug/wt");
    path
}

pub fn create_path_file(dir: &Path) -> PathBuf {
    dir.join(".wt-path")
}

pub fn read_path_file(path_file: &Path) -> String {
    std::fs::read_to_string(path_file).unwrap_or_default()
}

pub fn setup_git_repo(dir: &Path) {
    Command::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .expect("git init failed");

    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(dir)
        .output()
        .expect("git config email failed");

    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(dir)
        .output()
        .expect("git config name failed");

    std::fs::write(dir.join("README.md"), "# Test Repo\n").unwrap();

    Command::new("git")
        .args(["add", "."])
        .current_dir(dir)
        .output()
        .expect("git add failed");

    Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(dir)
        .output()
        .expect("git commit failed");

    Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(dir)
        .output()
        .ok();
}

pub fn setup_git_repo_with_home(dir: &Path) -> PathBuf {
    setup_git_repo(dir);

    let home = dir.join("fake_home");
    std::fs::create_dir_all(&home).unwrap();

    let wt_dir = home.join(".agent-worktree");
    std::fs::create_dir_all(&wt_dir).unwrap();

    let config = r#"
[worktree]
default_base = "main"
"#;
    std::fs::write(wt_dir.join("config.toml"), config).unwrap();

    home
}

pub fn setup_worktree_test_env() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();

    setup_git_repo(&repo);

    let home = dir.path().join("home");
    let wt_dir = home.join(".agent-worktree");
    std::fs::create_dir_all(&wt_dir).unwrap();

    let config = r#"
[worktree]
default_base = "main"
"#;
    std::fs::write(wt_dir.join("config.toml"), config).unwrap();

    (dir, repo, home)
}

/// Create a sub-repo and register it as a submodule of `repo` at `sub/`.
///
/// Newer git blocks file-protocol submodule clones by default, and the
/// restriction is NOT lifted by repo-local config (the clone subprocess
/// doesn't read it). Any command that triggers a submodule clone — including
/// the wt binary — must run with GIT_ALLOW_PROTOCOL=file.
pub fn add_submodule(parent: &Path, repo: &Path) {
    let sub_repo = parent.join("subrepo");
    std::fs::create_dir_all(&sub_repo).unwrap();
    setup_git_repo(&sub_repo);

    let output = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            sub_repo.to_str().unwrap(),
            "sub",
        ])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git submodule add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    Command::new("git")
        .args(["commit", "-m", "add submodule"])
        .current_dir(repo)
        .output()
        .unwrap();
}

/// A worktree whose checkout has an initialised submodule at `sub/`.
///
/// Returns `(tempdir, repo, home, worktree path)`; the tempdir binding must
/// stay alive for the duration of the test.
pub fn new_worktree_with_submodule(branch: &str) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let (dir, repo, home) = setup_worktree_test_env();
    add_submodule(dir.path(), &repo);

    let worktree = new_worktree(dir.path(), &repo, &home, branch, true);
    (dir, repo, home, worktree)
}

/// Run `wt new <branch>` in `repo` and return the created worktree path.
///
/// `allow_file_protocol` is required whenever the repo has submodules: git
/// refuses the `file` transport in the clone subprocess without it.
pub fn new_worktree(
    tmp: &Path,
    repo: &Path,
    home: &Path,
    branch: &str,
    allow_file_protocol: bool,
) -> PathBuf {
    let path_file = create_path_file(tmp);
    let mut command = Command::new(wt_binary());
    command
        .args(["new", branch, "--path-file", path_file.to_str().unwrap()])
        .current_dir(repo)
        .env("HOME", home);
    if allow_file_protocol {
        command.env("GIT_ALLOW_PROTOCOL", "file");
    }

    let output = command.output().expect("wt new failed");
    assert!(
        output.status.success(),
        "wt new failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    PathBuf::from(read_path_file(&path_file).trim())
}

/// A worktree directory git has lost all record of.
///
/// Reproduces the real-world trigger: the repository is re-created under the
/// same path, so its administrative worktree entries are gone while the pool
/// directory — keyed on that unchanged path — survives from the old repo.
///
/// Returns `(tempdir, repo, home, orphaned path)`; keep the tempdir alive.
pub fn orphaned_worktree_dir(branch: &str) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let (dir, repo, home) = setup_worktree_test_env();
    let worktree = new_worktree(dir.path(), &repo, &home, branch, false);

    std::fs::remove_dir_all(repo.join(".git/worktrees")).unwrap();
    Command::new("git")
        .args(["branch", "-D", branch])
        .current_dir(&repo)
        .output()
        .unwrap();

    assert!(worktree.exists(), "orphan setup lost the directory");
    (dir, repo, home, worktree)
}

/// Local branch names in `repo`.
pub fn local_branches(repo: &Path) -> Vec<String> {
    let output = Command::new("git")
        .args(["for-each-ref", "--format=%(refname:short)", "refs/heads/"])
        .current_dir(repo)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.to_string())
        .collect()
}
