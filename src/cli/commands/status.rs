// ===========================================================================
// wt status - Show current worktree information
// ===========================================================================

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use clap::Args;
use serde::Serialize;

use crate::cli::{Error, Result};
use crate::config::Config;
use crate::git;
use crate::meta::{self, WorktreeMeta};

#[derive(Args)]
pub struct StatusArgs {
    /// Output machine-readable JSON to stdout
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: StatusArgs, config: &Config) -> Result<()> {
    let current = git::current_branch()?;
    let workspace_id = git::workspace_id()?;
    let wt_dir = config.workspaces_dir.join(&workspace_id);
    let wt_path = wt_dir.join(&current);

    if !wt_path.exists() {
        return Err(Error::Other(format!(
            "Not in a managed worktree (branch: {current})"
        )));
    }

    let trunk = config.resolve_trunk();

    let meta_path = meta::meta_path_with_fallback(&wt_dir, &current);
    let loaded = WorktreeMeta::load(&meta_path).ok();

    let base_branch = loaded.as_ref().map(|m| m.base_branch.as_str());
    let effective_target = meta::resolve_target_branch(
        None,
        base_branch,
        |b| git::branch_exists(b).unwrap_or(false),
        &trunk,
    );

    let uncommitted = git::uncommitted_count_in(&wt_path).unwrap_or(0);
    let commits = git::commit_count(&effective_target, &current).unwrap_or(0);

    let diff = git::diff_shortstat(&effective_target, &current).unwrap_or(git::DiffStat {
        insertions: 0,
        deletions: 0,
    });
    let unstaged = git::diff_shortstat_in(&wt_path).unwrap_or(git::DiffStat {
        insertions: 0,
        deletions: 0,
    });

    let total_ins = diff.insertions + unstaged.insertions;
    let total_del = diff.deletions + unstaged.deletions;

    // Detect in-progress sync state (git-native only, no WT_MERGE_BRANCH)
    let sync_in_progress = if git::is_rebase_in_progress() {
        Some("rebase")
    } else if git::is_merge_in_progress() {
        Some("merge")
    } else {
        None
    };

    if args.json {
        // Versioned envelope: the schema is a public API for dashboards/scripts.
        let doc = StatusJson {
            version: 1,
            branch: &current,
            base_branch,
            trunk: &trunk,
            merge_target: &effective_target,
            created_at: loaded.as_ref().map(|m| m.created_at),
            commits,
            uncommitted,
            insertions: total_ins,
            deletions: total_del,
            path: &wt_path,
            sync_in_progress,
        };
        let json = serde_json::to_string(&doc)
            .map_err(|e| Error::Other(format!("failed to serialize JSON: {e}")))?;
        println!("{json}");
        return Ok(());
    }

    println!("Branch:       {current}");

    if let Some(bb) = base_branch {
        println!("Base branch:  {bb}");
    }

    println!("Trunk:        {trunk}");
    println!("Merge target: {effective_target}");

    if let Some(ref m) = loaded {
        println!(
            "Created:      {}",
            m.created_at.format("%Y-%m-%d %H:%M:%S UTC")
        );
    }

    println!("Commits:      {commits}");
    println!("Uncommitted:  {uncommitted}");

    if total_ins > 0 || total_del > 0 {
        println!("Diff:         +{total_ins} -{total_del}");
    } else {
        println!("Diff:         -");
    }

    println!("Path:         {}", wt_path.display());

    if let Some(kind) = sync_in_progress {
        println!();
        println!("State:        {} IN PROGRESS (sync)", kind.to_uppercase());
        println!("  Resolve conflicts, then: wt sync --continue");
        println!("  Or abort: wt sync --abort");
    }

    Ok(())
}

#[derive(Serialize)]
struct StatusJson<'a> {
    version: u32,
    branch: &'a str,
    base_branch: Option<&'a str>,
    trunk: &'a str,
    merge_target: &'a str,
    created_at: Option<DateTime<Utc>>,
    commits: usize,
    uncommitted: usize,
    insertions: usize,
    deletions: usize,
    path: &'a PathBuf,
    sync_in_progress: Option<&'a str>,
}
