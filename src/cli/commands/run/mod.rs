// ===========================================================================
// wt run - Run engine: create worktree, run agent, dispatch on outcome
//
// One engine, two dispatch strategies (see ARCHITECTURE.md "Run 引擎"):
// - headless (default): policy declared upfront via --on-success, exit-code
//   contract + --json for CI/orchestrators
// - interactive (-i): prompt after the agent exits, reopen loop in-process
// ===========================================================================

pub mod headless;
pub mod interactive;

use std::path::Path;

use clap::{Args, ValueEnum};
use clap_complete::engine::ArgValueCompleter;

use crate::cli::Result;
use crate::complete;
use crate::config::{Config, MergeStrategy};
use crate::git;
use crate::meta;
use crate::process;

use super::lifecycle::new::{create_worktree_core, CreatedWorktree};
use super::merge as merge_cmd;

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OnSuccess {
    /// Keep the worktree for manual review
    Keep,
    /// Merge into the base branch and remove the worktree
    Merge,
}

#[derive(Args)]
pub struct RunArgs {
    /// Branch name (random name like 'swift-fox' if not provided)
    branch: Option<String>,

    /// Base branch to create from and merge back to (default: current branch)
    #[arg(long, value_name = "BRANCH", add = ArgValueCompleter::new(complete::complete_branches))]
    base: Option<String>,

    /// Interactive dispatch: prompt merge/reopen/quit after the agent exits
    #[arg(short = 'i', long, conflicts_with_all = ["on_success", "json"])]
    interactive: bool,

    /// What to do when the agent exits 0 leaving only commits (default: keep)
    #[arg(long, value_enum, default_value = "keep")]
    on_success: OnSuccess,

    /// Merge strategy when merging (default: from config)
    #[arg(short, long, value_enum)]
    strategy: Option<MergeStrategy>,

    /// Skip pre-merge hooks
    #[arg(short = 'H', long)]
    skip_hooks: bool,

    /// Output a JSON result to stdout (agent stdout is diverted to stderr)
    #[arg(long)]
    json: bool,

    /// Agent command (after `--`), executed directly without a shell
    #[arg(last = true, required = true, value_name = "CMD", num_args = 1..)]
    command: Vec<String>,
}

pub fn run(args: RunArgs, config: &Config, path_file: Option<&Path>) -> Result<()> {
    if args.interactive {
        // Guard before creating anything, mirroring the old snap constraint.
        interactive::refuse_inside_workspace(config)?;
    }

    let created = create_worktree_core(args.branch.clone(), args.base.as_deref(), config)?;
    let strategy = args.strategy.unwrap_or(config.merge_strategy);

    if args.interactive {
        interactive::run_session(
            &created,
            &args.command,
            strategy,
            args.skip_hooks,
            config,
            path_file,
        )
    } else {
        headless::run(args, &created, strategy, config)
    }
}

// ===========================================================================
// Shared post-agent state
// ===========================================================================

/// Git facts gathered after the agent exits; both dispatch strategies decide
/// from this same snapshot.
pub struct WorktreeState {
    pub uncommitted: usize,
    pub commits: usize,
    pub base_exists: bool,
    pub merge_target: String,
}

/// Merge target: base branch while it still exists; trunk is report-only
/// fallback. Merging into trunk when the base was deleted mid-run would land
/// commits on the wrong branch, so dispatchers refuse that case explicitly.
pub fn gather_state(created: &CreatedWorktree, config: &Config) -> WorktreeState {
    let uncommitted = git::uncommitted_count_in(&created.path).unwrap_or(0);
    let base_exists = git::branch_exists(&created.base_branch).unwrap_or(false);
    let merge_target = if base_exists {
        created.base_branch.clone()
    } else {
        config.resolve_trunk()
    };
    let commits = git::commit_count(&merge_target, &created.branch).unwrap_or(0);
    WorktreeState {
        uncommitted,
        commits,
        base_exists,
        merge_target,
    }
}

/// Remove worktree, branch, and metadata after a no-changes run.
///
/// Uses non-force removal so that any untracked files left in the worktree
/// (build artifacts, .env, agent-generated scratch) cause the cleanup to
/// fail loudly instead of silently deleting work.
pub fn cleanup_worktree(wt_path: &Path, branch: &str, config: &Config) -> Result<()> {
    git::remove_worktree(wt_path, false)?;
    git::delete_branch(branch, true).ok();

    if let Ok(workspace_id) = git::workspace_id() {
        let wt_dir = config.workspaces_dir.join(&workspace_id);
        meta::remove_meta(&wt_dir, branch);
    }

    Ok(())
}

// ===========================================================================
// Shared merge pipeline
// ===========================================================================

/// Terminal state of the merge pipeline. Dispatchers map this to their own
/// contract (exit codes / JSON for headless, prompts and cd for interactive)
/// — the orchestration itself must not fork per strategy.
pub enum MergeOutcome {
    /// Merge landed. `worktree_removed: false` means post-merge cleanup
    /// failed and the worktree was left for inspection.
    Merged { worktree_removed: bool },
    /// Already up to date: nothing landed, worktree kept.
    NothingToMerge,
    /// Dry-run detected conflicts; HEAD restored, worktree preserved.
    Conflict,
    /// A pre_merge (merge not attempted) or post_merge (merge landed) hook
    /// failed; worktree preserved.
    HookFailed,
    /// Any other failure; HEAD restored, worktree preserved.
    Failed,
}

/// Merge the run's branch into `merge_target`, mirroring `wt merge` atomic
/// semantics: any failure restores main-repo HEAD and preserves the worktree.
pub fn merge_pipeline(
    created: &CreatedWorktree,
    merge_target: &str,
    strategy: MergeStrategy,
    skip_hooks: bool,
    config: &Config,
) -> MergeOutcome {
    let hook_env = process::HookEnv {
        main_repo: &created.repo_root,
        worktree: &created.path,
        branch: &created.branch,
        base_branch: merge_target,
    };

    if !skip_hooks && !config.hooks.pre_merge.is_empty() {
        eprintln!("Running pre-merge hooks...");
        if let Err(e) = process::run_hooks(&config.hooks.pre_merge, &created.path, &hook_env) {
            eprintln!("pre_merge hook failed: {e}. Worktree preserved.");
            return MergeOutcome::HookFailed;
        }
    }

    if std::env::set_current_dir(&created.repo_root).is_err() {
        eprintln!("Cannot enter main repo. Worktree preserved.");
        return MergeOutcome::Failed;
    }
    if git::has_uncommitted_changes().unwrap_or(true) {
        eprintln!("Main repo has uncommitted changes. Worktree preserved.");
        return MergeOutcome::Failed;
    }
    if git::is_merge_in_progress() || git::is_rebase_in_progress() {
        eprintln!("Main repo has a merge/rebase in progress. Worktree preserved.");
        return MergeOutcome::Failed;
    }

    // Capture main repo's current branch *before* moving HEAD, so any
    // failure below can restore it.
    let original_main_branch = git::current_branch().ok();
    let restore = |msg: &str| {
        if let Some(orig) = &original_main_branch {
            let _ = git::checkout(orig);
        }
        eprintln!("{msg}");
    };

    if git::checkout(merge_target).is_err() {
        restore("Cannot checkout merge target. Worktree preserved.");
        return MergeOutcome::Failed;
    }

    match git::dry_run_merge(&created.branch, strategy.is_squash()) {
        Ok(true) => {}
        Ok(false) => {
            restore("Merge would conflict. Worktree preserved.");
            return MergeOutcome::Conflict;
        }
        Err(e) => {
            restore(&format!("Merge pre-check failed: {e}. Worktree preserved."));
            return MergeOutcome::Failed;
        }
    }

    match merge_cmd::execute_merge(&created.branch, merge_target, strategy) {
        Ok(true) => {}
        Ok(false) => {
            restore("Nothing to merge: already up to date. Worktree kept.");
            return MergeOutcome::NothingToMerge;
        }
        Err(e) => {
            let _ = git::reset_merge();
            restore(&format!("Merge failed: {e}. Worktree preserved."));
            return MergeOutcome::Failed;
        }
    }

    eprintln!("Merged {} into {merge_target}", created.branch);

    // Merge has already landed at this point: report hook failure but keep
    // the worktree so the user can inspect what post_merge saw.
    if !config.hooks.post_merge.is_empty() {
        eprintln!("Running post-merge hooks...");
        if let Err(e) = process::run_hooks(&config.hooks.post_merge, &created.path, &hook_env) {
            eprintln!("post_merge hook failed: {e}. Merge landed; worktree preserved.");
            return MergeOutcome::HookFailed;
        }
    }

    if let Err(e) = merge_cmd::cleanup_worktree(&created.branch, config) {
        eprintln!("Cleanup failed: {e}. Merge landed; worktree preserved.");
        return MergeOutcome::Merged {
            worktree_removed: false,
        };
    }

    MergeOutcome::Merged {
        worktree_removed: true,
    }
}
