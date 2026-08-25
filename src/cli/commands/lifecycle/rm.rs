// ===========================================================================
// wt rm - Remove a worktree
// ===========================================================================

use std::path::Path;

use clap::Args;
use clap_complete::engine::ArgValueCompleter;

use crate::cli::{write_path_file, Error, Result};
use crate::complete;
use crate::config::Config;
use crate::git;
use crate::meta;

#[derive(Args)]
pub struct RmArgs {
    /// Branch name to remove (use '.' for current worktree)
    #[arg(add = ArgValueCompleter::new(complete::complete_worktrees))]
    branch: String,

    /// Force removal even with uncommitted changes
    #[arg(short, long)]
    force: bool,
}

pub fn run(args: RmArgs, config: &Config, path_file: Option<&Path>) -> Result<()> {
    // Get main repo path BEFORE any destructive operations
    let main_path = git::repo_root()?;
    let workspace_id = git::workspace_id()?;
    let wt_dir = config.workspaces_dir.join(&workspace_id);

    // Resolve '.' to current branch
    let branch = if args.branch == "." {
        git::current_branch()?
    } else {
        args.branch
    };

    let wt_path = wt_dir.join(&branch);

    // Stale = a directory git has no record of; see `remove_stale_worktree_dir`.
    let registered = git::is_registered_worktree(&wt_path)?;
    if !registered && !wt_path.exists() {
        // The tree is gone; its metadata may not be. Metadata is what `wt ls`
        // and completions read, so it is the only witness that the name ever
        // existed — nothing left to clear means nothing was ever here.
        if !meta::remove_meta(&wt_dir, &branch) {
            return Err(Error::Git(git::Error::WorktreeNotFound(branch)));
        }
        std::env::set_current_dir(&main_path).ok();
        let _ = git::delete_branch(&branch, args.force);
        eprintln!("Removed stale metadata: {branch}");
        return Ok(());
    }

    // Check if we're inside the worktree being removed
    let inside_target = git::is_cwd_inside(&wt_path);

    // Without the shell wrapper, removing the current worktree leaves the
    // parent shell stranded in a deleted directory (every subsequent `pwd`
    // / `ls` then errors). Refuse instead of producing a broken shell.
    if inside_target && path_file.is_none() {
        return Err(Error::Other(
            "Refusing to remove the current worktree without shell integration.\n\
             Run 'wt setup' first, or 'cd' to the main repo and retry."
                .into(),
        ));
    }

    if registered {
        git::remove_worktree(&wt_path, args.force)?;
    } else {
        // A stale tree can hold work no one can account for — git can't say
        // whether it is dirty, because it doesn't recognize it at all. So it
        // is never removed implicitly; --force is the user vouching for it.
        if !args.force {
            return Err(Error::Git(git::Error::WorktreeStale {
                branch,
                path: wt_path.display().to_string(),
            }));
        }
        git::remove_stale_worktree_dir(&wt_path)?;
    }

    // Switch to main repo before deleting branch (avoid "not in repo" error)
    std::env::set_current_dir(&main_path).ok();

    // Delete branch — best-effort, failure doesn't block worktree cleanup
    let _ = git::delete_branch(&branch, args.force);

    // Remove metadata
    meta::remove_meta(&wt_dir, &branch);

    eprintln!("Removed worktree: {branch}");

    // If we were inside the removed worktree, write main repo path for shell to cd
    if path_file.is_some() && inside_target {
        write_path_file(path_file, &main_path)?;
    }

    Ok(())
}
