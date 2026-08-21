// ===========================================================================
// git/worktree - Worktree CRUD
// ===========================================================================

use std::path::{Path, PathBuf};
use std::process::Command;

use super::{path_str, run, submodule, uncommitted_count_in, Error, Result};

/// Create a new worktree
pub fn create_worktree(path: &Path, branch: &str, base: &str) -> Result<()> {
    let path_str = path_str(path)?;

    // Check if branch already exists
    if super::branch_exists(branch)? {
        // Branch exists - check if it already has a worktree
        let worktrees = list_worktrees()?;
        if worktrees
            .iter()
            .any(|wt| wt.branch.as_deref() == Some(branch))
        {
            return Err(Error::WorktreeExists(branch.to_string()));
        }

        // Branch exists but no worktree - just check it out
        run(&["worktree", "add", path_str, branch])?;
    } else {
        // Branch doesn't exist - create it from base
        run(&["worktree", "add", "-b", branch, path_str, base])?;
    }

    Ok(())
}

/// Remove a worktree.
///
/// `--force` is passed unconditionally: `git worktree remove` refuses outright
/// on any worktree containing submodules, and forcing is the only bypass git
/// offers. That same flag also disables git's "modified or untracked files"
/// guard, so the guard is re-implemented here — without it a non-force removal
/// would silently discard work.
pub fn remove_worktree(path: &Path, force: bool) -> Result<()> {
    if !force && uncommitted_count_in(path)? > 0 {
        // The path, not its last component: branch names contain '/', so
        // 'feat/x' would report as 'x'.
        return Err(Error::WorktreeDirty(path.display().to_string()));
    }

    run(&["worktree", "remove", "--force", path_str(path)?])
}

/// Move a worktree to a new path.
///
/// `git worktree move` also dies on worktrees containing submodules, but there
/// `--force` is no escape hatch, so that case is done by hand: rename the
/// directory, let `git worktree repair` fix git's back-pointer, then re-point
/// every submodule — their `core.worktree` and `.git` gitfile still describe
/// the old location.
pub fn move_worktree(old_path: &Path, new_path: &Path) -> Result<()> {
    let submodules = submodule::gitdirs_in(old_path)?;
    if submodules.is_empty() {
        return run(&["worktree", "move", path_str(old_path)?, path_str(new_path)?]);
    }

    // Resolve before the rename: afterwards the old path is gone, and the
    // recorded core.worktree values are relative to its resolved location.
    let old_real = old_path.canonicalize()?;
    std::fs::rename(old_path, new_path)?;

    if let Err(e) = repair_moved_worktree(new_path, &old_real, &submodules) {
        // Put the tree back where git still believes it lives.
        std::fs::rename(new_path, old_path).ok();
        run(&["-C", path_str(old_path)?, "worktree", "repair"]).ok();
        return Err(e);
    }

    Ok(())
}

/// Point git and every submodule at the worktree's new location.
///
/// Kept apart from `move_worktree` as its error boundary: every step here is
/// fallible, and each failure needs the same rename-back rollback.
fn repair_moved_worktree(new_path: &Path, old_real: &Path, submodules: &[PathBuf]) -> Result<()> {
    let new_real = new_path.canonicalize()?;

    // Run from inside the moved tree: repair then finds both ends of the
    // broken link itself, so no path argument is needed.
    run(&["-C", path_str(&new_real)?, "worktree", "repair"])?;

    submodule::repoint_all(submodules, old_real, &new_real)
}

/// List all worktrees
pub fn list_worktrees() -> Result<Vec<WorktreeInfo>> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .output()?;

    if !output.status.success() {
        return Err(Error::NotInRepo);
    }

    let content = String::from_utf8_lossy(&output.stdout);
    Ok(parse_worktree_list(&content))
}

/// Parse git worktree list --porcelain output
pub fn parse_worktree_list(content: &str) -> Vec<WorktreeInfo> {
    let mut worktrees = Vec::new();
    let mut current: Option<WorktreeInfo> = None;

    for line in content.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(wt) = current.take() {
                worktrees.push(wt);
            }
            current = Some(WorktreeInfo {
                path: PathBuf::from(path),
                branch: None,
                commit: None,
                is_bare: false,
            });
        } else if let Some(ref mut wt) = current {
            if let Some(branch) = line.strip_prefix("branch refs/heads/") {
                wt.branch = Some(branch.to_string());
            } else if let Some(commit) = line.strip_prefix("HEAD ") {
                wt.commit = Some(commit.to_string());
            } else if line == "bare" {
                wt.is_bare = true;
            }
        }
    }

    if let Some(wt) = current {
        worktrees.push(wt);
    }

    worktrees
}

#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub is_bare: bool,
}
