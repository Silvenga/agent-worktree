// ===========================================================================
// git - Git Operations via CLI
// ===========================================================================

mod branch;
mod ops;
mod repo;
mod submodule;
mod worktree;

pub use branch::*;
pub use ops::*;
pub use repo::*;
pub use submodule::*;
pub use worktree::*;

use std::path::{Path, PathBuf};
use std::process::Command;

pub type Result<T> = std::result::Result<T, Error>;

/// Resolve a path for comparison against one git reported.
///
/// Canonicalizing fails on a path that no longer exists; the raw value is the
/// fallback, since nothing resolved can equal a path that isn't there anyway.
pub(crate) fn real_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Safe Path -> &str conversion, replacing .to_str().unwrap()
pub(crate) fn path_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| Error::Command(format!("path contains invalid UTF-8: {}", path.display())))
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Command(String),

    #[error("not in a git repository")]
    NotInRepo,

    #[error("worktree '{0}' not found")]
    WorktreeNotFound(String),

    #[error("worktree '{0}' already exists")]
    WorktreeExists(String),

    #[error("worktree '{0}' has uncommitted changes, use --force")]
    WorktreeDirty(String),

    #[error(
        "'{path}' exists but git does not know it as a worktree \
         (leftover from an earlier repository at this path?); \
         run 'wt rm --force {branch}' to discard it"
    )]
    WorktreeStale { branch: String, path: String },

    #[error("branch '{0}' not found")]
    BranchNotFound(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Extract error message from git command output.
///
/// Some git commands (merge, commit) put error info in stdout, not stderr.
/// This function checks stderr first, falls back to stdout.
fn extract_error(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.trim().is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        clean_git_error(&stderr)
    }
}

/// Clean git stderr to user-friendly message
fn clean_git_error(stderr: &str) -> String {
    let msg = stderr.trim();

    // Remove "fatal: " or "error: " prefix
    let msg = msg
        .strip_prefix("fatal: ")
        .or_else(|| msg.strip_prefix("error: "))
        .unwrap_or(msg);

    msg.to_string()
}

/// Run a git command; on failure extract the error from stderr+stdout
fn run(args: &[&str]) -> Result<()> {
    let output = Command::new("git").args(args).output()?;
    if !output.status.success() {
        return Err(Error::Command(extract_error(&output)));
    }
    Ok(())
}

/// Run a git command and return its trimmed stdout
fn run_output(args: &[&str]) -> Result<String> {
    let output = Command::new("git").args(args).output()?;
    if !output.status.success() {
        return Err(Error::Command(extract_error(&output)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Read one git config value; `None` when unset, empty, or unreadable.
///
/// `file` pins the lookup to an explicit config file. That is mandatory for a
/// submodule's own config: a plain `git config` first chdirs into
/// `core.worktree`, which during a repair is exactly the stale path being
/// fixed, and dies there.
fn config_value(file: Option<&Path>, key: &str) -> Option<String> {
    let mut args = vec!["config"];
    if let Some(file) = file {
        args.push("--file");
        args.push(file.to_str()?);
    }
    args.push("--get");
    args.push(key);

    let output = Command::new("git").args(&args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests;
