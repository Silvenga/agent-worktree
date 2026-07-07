// ===========================================================================
// wt run (headless) - Policy dispatch: --on-success declared upfront
// ===========================================================================

use std::io::Write;
use std::path::Path;

use serde::Serialize;

use crate::cli::{Error, Result};
use crate::config::{Config, MergeStrategy};
use crate::process;

use super::super::lifecycle::new::CreatedWorktree;
use super::{cleanup_worktree, gather_state, merge_pipeline, MergeOutcome, OnSuccess, RunArgs};

// Exit codes are the machine contract for CI/orchestrators. They start at 10
// to stay clear of generic failures (1). Keep in sync with README and
// ARCHITECTURE.md. Interactive dispatch does not use them (human already
// decided — it exits 0).
pub const EXIT_AGENT_FAILED: i32 = 10;
pub const EXIT_UNCOMMITTED: i32 = 11;
pub const EXIT_HOOK_FAILED: i32 = 12;
pub const EXIT_MERGE_CONFLICT: i32 = 13;

/// Terminal state of a headless `wt run`. One-to-one with exit codes so
/// scripts can branch on either the JSON `outcome` or the process status.
struct RunOutcome {
    outcome: &'static str,
    exit_code: i32,
    worktree_removed: bool,
}

pub fn run(
    args: RunArgs,
    created: &CreatedWorktree,
    strategy: MergeStrategy,
    config: &Config,
) -> Result<()> {
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

    let state = gather_state(created, config);

    let result = if agent_exit_code != 0 {
        eprintln!("Agent exited with code {agent_exit_code}. Worktree preserved.");
        RunOutcome {
            outcome: "agent_failed",
            exit_code: EXIT_AGENT_FAILED,
            worktree_removed: false,
        }
    } else if state.uncommitted > 0 {
        eprintln!(
            "Agent left {} uncommitted change(s). Worktree preserved.",
            state.uncommitted
        );
        eprintln!(
            "Commit manually, then 'wt merge' or 'wt rm {}'.",
            created.branch
        );
        RunOutcome {
            outcome: "uncommitted",
            exit_code: EXIT_UNCOMMITTED,
            worktree_removed: false,
        }
    } else if state.commits == 0 {
        eprintln!("No changes detected. Cleaning up...");
        cleanup_worktree(&created.path, &created.branch, config)?;
        RunOutcome {
            outcome: "no_changes",
            exit_code: 0,
            worktree_removed: true,
        }
    } else {
        match args.on_success {
            OnSuccess::Keep => {
                eprintln!(
                    "Agent done: {} commit(s) on '{}'. Worktree kept.",
                    state.commits, created.branch
                );
                RunOutcome {
                    outcome: "kept",
                    exit_code: 0,
                    worktree_removed: false,
                }
            }
            OnSuccess::Merge if !state.base_exists => {
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
                let outcome = merge_pipeline(
                    created,
                    &state.merge_target,
                    strategy,
                    args.skip_hooks,
                    config,
                );
                if let MergeOutcome::Conflict = outcome {
                    eprintln!(
                        "Resolve in the worktree: wt cd {} && wt sync && wt merge",
                        created.branch
                    );
                }
                map_merge_outcome(outcome)
            }
        }
    };

    if args.json {
        let doc = RunJson {
            version: 1,
            branch: &created.branch,
            path: &created.path,
            base_branch: &created.base_branch,
            merge_target: &state.merge_target,
            agent_exit_code,
            commits: state.commits,
            uncommitted: state.uncommitted,
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

fn map_merge_outcome(outcome: MergeOutcome) -> RunOutcome {
    match outcome {
        MergeOutcome::Merged { worktree_removed } => RunOutcome {
            outcome: "merged",
            exit_code: 0,
            worktree_removed,
        },
        MergeOutcome::NothingToMerge => RunOutcome {
            outcome: "kept",
            exit_code: 0,
            worktree_removed: false,
        },
        MergeOutcome::Conflict => RunOutcome {
            outcome: "merge_conflict",
            exit_code: EXIT_MERGE_CONFLICT,
            worktree_removed: false,
        },
        MergeOutcome::HookFailed => RunOutcome {
            outcome: "hook_failed",
            exit_code: EXIT_HOOK_FAILED,
            worktree_removed: false,
        },
        MergeOutcome::Failed => RunOutcome {
            outcome: "merge_failed",
            exit_code: EXIT_MERGE_CONFLICT,
            worktree_removed: false,
        },
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
