// ===========================================================================
// wt run - Headless snap: create worktree, run agent, dispatch on outcome
// ===========================================================================

use std::io::Write;
use std::path::Path;

use clap::{Args, ValueEnum};
use clap_complete::engine::ArgValueCompleter;
use serde::Serialize;

use crate::cli::{Error, Result};
use crate::complete;
use crate::config::{Config, MergeStrategy};
use crate::git;
use crate::process;

use super::lifecycle::new::{create_worktree_core, CreatedWorktree};
use super::merge as merge_cmd;
use super::snap::resume;

// Exit codes are the machine contract for CI/orchestrators. They start at 10
// to stay clear of generic failures (1) and the snap shell-wrapper codes
// (0/2/3 in snap/resume.rs). Keep in sync with README and ARCHITECTURE.md.
pub const EXIT_AGENT_FAILED: i32 = 10;
pub const EXIT_UNCOMMITTED: i32 = 11;
pub const EXIT_HOOK_FAILED: i32 = 12;
pub const EXIT_MERGE_CONFLICT: i32 = 13;

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

    /// What to do when the agent exits 0 leaving only commits (default: keep)
    #[arg(long, value_enum, default_value = "keep")]
    on_success: OnSuccess,

    /// Merge strategy for --on-success merge (default: from config)
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

/// Terminal state of a `wt run` invocation. One-to-one with exit codes so
/// scripts can branch on either the JSON `outcome` or the process status.
struct RunOutcome {
    outcome: &'static str,
    exit_code: i32,
    worktree_removed: bool,
}

pub fn run(args: RunArgs, config: &Config) -> Result<()> {
    let created = create_worktree_core(args.branch.clone(), args.base.as_deref(), config)?;

    let agent_env = process::HookEnv {
        main_repo: &created.repo_root,
        worktree: &created.path,
        branch: &created.branch,
        base_branch: &created.base_branch,
    };

    eprintln!(
        "Running agent in worktree '{}' ({})...",
        created.branch,
        created.path.display()
    );
    let status = process::run_agent(&args.command, &created.path, &agent_env, args.json)
        .map_err(|e| Error::Other(format!("failed to run agent: {e}")))?;
    // Signal-terminated agents have no code; report -1 rather than lying with 0.
    let agent_exit_code = status.code().unwrap_or(-1);

    let uncommitted = git::uncommitted_count_in(&created.path).unwrap_or(0);

    // Merge target: base branch while it still exists; trunk is report-only
    // fallback. Merging into trunk when the base was deleted mid-run would
    // land commits on the wrong branch, so that case refuses below.
    let base_exists = git::branch_exists(&created.base_branch).unwrap_or(false);
    let merge_target = if base_exists {
        created.base_branch.clone()
    } else {
        config.resolve_trunk()
    };
    let commits = git::commit_count(&merge_target, &created.branch).unwrap_or(0);

    let result = if agent_exit_code != 0 {
        eprintln!("Agent exited with code {agent_exit_code}. Worktree preserved.");
        RunOutcome {
            outcome: "agent_failed",
            exit_code: EXIT_AGENT_FAILED,
            worktree_removed: false,
        }
    } else if uncommitted > 0 {
        eprintln!("Agent left {uncommitted} uncommitted change(s). Worktree preserved.");
        eprintln!(
            "Commit manually, then 'wt merge' or 'wt rm {}'.",
            created.branch
        );
        RunOutcome {
            outcome: "uncommitted",
            exit_code: EXIT_UNCOMMITTED,
            worktree_removed: false,
        }
    } else if commits == 0 {
        eprintln!("No changes detected. Cleaning up...");
        resume::cleanup_worktree(&created.path, &created.branch, config)?;
        RunOutcome {
            outcome: "no_changes",
            exit_code: 0,
            worktree_removed: true,
        }
    } else {
        match args.on_success {
            OnSuccess::Keep => {
                eprintln!(
                    "Agent done: {commits} commit(s) on '{}'. Worktree kept.",
                    created.branch
                );
                RunOutcome {
                    outcome: "kept",
                    exit_code: 0,
                    worktree_removed: false,
                }
            }
            OnSuccess::Merge if !base_exists => {
                eprintln!(
                    "Base branch '{}' no longer exists. Worktree preserved.",
                    created.base_branch
                );
                eprintln!("Resolve manually with: wt merge --into <branch>");
                RunOutcome {
                    outcome: "merge_failed",
                    exit_code: EXIT_MERGE_CONFLICT,
                    worktree_removed: false,
                }
            }
            OnSuccess::Merge => {
                let strategy = args.strategy.unwrap_or(config.merge_strategy);
                merge_and_cleanup(&created, &merge_target, strategy, args.skip_hooks, config)
            }
        }
    };

    if args.json {
        let doc = RunJson {
            version: 1,
            branch: &created.branch,
            path: &created.path,
            base_branch: &created.base_branch,
            merge_target: &merge_target,
            agent_exit_code,
            commits,
            uncommitted,
            outcome: result.outcome,
            worktree_removed: result.worktree_removed,
            exit_code: result.exit_code,
        };
        let json = serde_json::to_string(&doc)
            .map_err(|e| Error::Other(format!("failed to serialize JSON: {e}")))?;
        println!("{json}");
        // process::exit skips destructors; a piped stdout is block-buffered
        // and would lose the JSON without this flush.
        let _ = std::io::stdout().flush();
    }

    if result.exit_code != 0 {
        std::process::exit(result.exit_code);
    }
    Ok(())
}

/// Merge the run's branch into `merge_target`, mirroring `wt merge` atomic
/// semantics: any failure restores main-repo HEAD and preserves the worktree.
fn merge_and_cleanup(
    created: &CreatedWorktree,
    merge_target: &str,
    strategy: MergeStrategy,
    skip_hooks: bool,
    config: &Config,
) -> RunOutcome {
    let preserved = RunOutcome {
        outcome: "merge_failed",
        exit_code: EXIT_MERGE_CONFLICT,
        worktree_removed: false,
    };
    let hook_failed = RunOutcome {
        outcome: "hook_failed",
        exit_code: EXIT_HOOK_FAILED,
        worktree_removed: false,
    };

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
            return hook_failed;
        }
    }

    if std::env::set_current_dir(&created.repo_root).is_err() {
        eprintln!("Cannot enter main repo. Worktree preserved.");
        return preserved;
    }
    if git::has_uncommitted_changes().unwrap_or(true) {
        eprintln!("Main repo has uncommitted changes. Worktree preserved.");
        return preserved;
    }
    if git::is_merge_in_progress() || git::is_rebase_in_progress() {
        eprintln!("Main repo has a merge/rebase in progress. Worktree preserved.");
        return preserved;
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
        return preserved;
    }

    match git::dry_run_merge(&created.branch, strategy.is_squash()) {
        Ok(true) => {}
        Ok(false) => {
            restore("Merge would conflict. Worktree preserved.");
            eprintln!(
                "Resolve in the worktree: wt cd {} && wt sync && wt merge",
                created.branch
            );
            return RunOutcome {
                outcome: "merge_conflict",
                ..preserved
            };
        }
        Err(e) => {
            restore(&format!("Merge pre-check failed: {e}. Worktree preserved."));
            return preserved;
        }
    }

    match merge_cmd::execute_merge(&created.branch, merge_target, strategy) {
        Ok(true) => {}
        Ok(false) => {
            restore("Nothing to merge: already up to date. Worktree kept.");
            return RunOutcome {
                outcome: "kept",
                exit_code: 0,
                worktree_removed: false,
            };
        }
        Err(e) => {
            let _ = git::reset_merge();
            restore(&format!("Merge failed: {e}. Worktree preserved."));
            return preserved;
        }
    }

    eprintln!("Merged {} into {merge_target}", created.branch);

    // Merge has already landed at this point: report hook failure but keep
    // the worktree so the user can inspect what post_merge saw.
    if !config.hooks.post_merge.is_empty() {
        eprintln!("Running post-merge hooks...");
        if let Err(e) = process::run_hooks(&config.hooks.post_merge, &created.path, &hook_env) {
            eprintln!("post_merge hook failed: {e}. Merge landed; worktree preserved.");
            return hook_failed;
        }
    }

    if let Err(e) = merge_cmd::cleanup_worktree(&created.branch, config) {
        eprintln!("Cleanup failed: {e}. Merge landed; worktree preserved.");
        return RunOutcome {
            outcome: "merged",
            exit_code: 0,
            worktree_removed: false,
        };
    }

    RunOutcome {
        outcome: "merged",
        exit_code: 0,
        worktree_removed: true,
    }
}

#[derive(Serialize)]
struct RunJson<'a> {
    version: u32,
    branch: &'a str,
    path: &'a Path,
    base_branch: &'a str,
    merge_target: &'a str,
    agent_exit_code: i32,
    commits: usize,
    uncommitted: usize,
    outcome: &'a str,
    worktree_removed: bool,
    exit_code: i32,
}
