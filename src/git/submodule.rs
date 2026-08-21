// ===========================================================================
// git/submodule - Per-worktree submodule gitdir layout
// ===========================================================================

use std::path::{Component, Path, PathBuf};
use std::process::Command;

use super::{config_value, path_str, run, run_output, Error, Result};

/// Initialize submodules inside a worktree, cloning `jobs` of them in parallel.
///
/// `git worktree add` leaves submodule directories empty; without this,
/// hooks and agents see an incomplete checkout. Git stores submodule gitdirs
/// per-worktree (`.git/worktrees/<wt>/modules/`), so nothing is reused from the
/// main repo — every submodule is re-cloned over the network, hence `jobs`.
///
/// Output is NOT captured: these clones are network-bound and can run for
/// minutes, so git's progress must reach the terminal. Swallowing it makes
/// "slow" indistinguishable from "hung". The failure path therefore reports
/// only the exit status — the real diagnostics are already on screen.
pub fn submodule_update_init(path: &Path, jobs: u32) -> Result<()> {
    let status = Command::new("git")
        .args([
            "-C",
            path_str(path)?,
            "submodule",
            "update",
            "--init",
            "--recursive",
            "--jobs",
            &jobs.to_string(),
        ])
        .status()?;

    if !status.success() {
        return Err(Error::Command(format!(
            "git submodule update failed ({status})"
        )));
    }
    Ok(())
}

/// Every submodule gitdir of `worktree`, at any depth (empty when there are none).
pub(super) fn gitdirs_in(worktree: &Path) -> Result<Vec<PathBuf>> {
    let gitdir = run_output(&["-C", path_str(worktree)?, "rev-parse", "--absolute-git-dir"])?;

    let mut found = Vec::new();
    scan(&Path::new(&gitdir).join("modules"), &mut found);
    Ok(found)
}

/// Walk `<gitdir>/modules`, which nests two different ways.
///
/// A submodule is named by its path, so `libs/foo` produces a bare namespace
/// directory `modules/libs/` holding no config; a submodule of a submodule
/// lives at `modules/<a>/modules/<b>`. Hence: config present -> leaf, recurse
/// into its own `modules`; absent -> namespace level, recurse in place.
fn scan(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        // file_type() rides along with the directory read; is_dir() on the
        // path would cost an extra stat per entry.
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }

        let path = entry.path();
        if path.join("config").is_file() {
            found.push(path.clone());
            scan(&path.join("modules"), found);
        } else {
            scan(&path, found);
        }
    }
}

/// Re-point submodules after their worktree was moved by hand.
///
/// Both halves of the link still describe the old location: the gitdir's
/// `core.worktree` and the checkout's `.git` gitfile.
pub(super) fn repoint_all(gitdirs: &[PathBuf], old_root: &Path, new_root: &Path) -> Result<()> {
    for gitdir in gitdirs {
        repoint(gitdir, old_root, new_root)?;
    }
    Ok(())
}

/// Rewrite one submodule's two-way link with its checkout.
///
/// Absolute paths are written on purpose: a branch name may contain '/', so a
/// rename can change the worktree's directory depth and silently invalidate
/// the relative links git wrote when the submodule was first checked out.
fn repoint(gitdir: &Path, old_root: &Path, new_root: &Path) -> Result<()> {
    let config = gitdir.join("config");
    let Some(recorded) = config_value(Some(&config), "core.worktree") else {
        // Never checked out in this worktree — nothing points anywhere.
        return Ok(());
    };

    // core.worktree is relative to the gitdir; joining resolves it, and an
    // absolute value simply replaces the base.
    let old_checkout = normalize_path(&gitdir.join(recorded));
    let Ok(relative) = old_checkout.strip_prefix(old_root) else {
        // Points outside the worktree being moved — not ours to rewrite.
        return Ok(());
    };
    let new_checkout = new_root.join(relative);

    run(&[
        "config",
        "--file",
        path_str(&config)?,
        "core.worktree",
        path_str(&new_checkout)?,
    ])?;

    std::fs::write(
        new_checkout.join(".git"),
        format!("gitdir: {}\n", path_str(gitdir)?),
    )?;

    Ok(())
}

/// Resolve `.` and `..` lexically.
///
/// `canonicalize` is unusable here: the path being resolved describes a
/// location that the move has already invalidated.
fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();

    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }

    out
}
