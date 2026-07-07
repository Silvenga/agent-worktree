// ===========================================================================
// wt run -i (interactive) - Prompt dispatch: human decides after the agent
// exits; reopen loop runs in-process (no shell wrapper involvement)
// ===========================================================================

use std::path::Path;

use crate::cli::{write_path_file, Error, Result};
use crate::config::{Config, MergeStrategy};
use crate::git;
use crate::process;
use crate::prompt::{self, ExitChoice, MergeChoice};

use super::super::lifecycle::new::CreatedWorktree;
use super::{cleanup_worktree, gather_state, merge_pipeline, MergeOutcome};

/// Action to take after the agent exits.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    /// Cleanup the worktree (no changes)
    CleanupNoChanges,
    /// Merge changes and cleanup
    MergeAndCleanup,
    /// Keep the worktree for manual handling
    ExitPreserve,
    /// Respawn the agent
    Reopen,
}

/// Nested interactive sessions stack confusingly: the new worktree would be
/// based on another worktree's branch and the final cd would strand the user.
pub(super) fn refuse_inside_workspace(config: &Config) -> Result<()> {
    let workspace_dir = config.workspaces_dir.join(&git::workspace_id()?);
    if git::is_cwd_inside(&workspace_dir) {
        return Err(Error::Other(
            "Refusing to start an interactive session inside an existing worktree.\n\
             Run 'wt cd' to return to the main repo, then retry."
                .into(),
        ));
    }
    Ok(())
}

/// Agent-to-dispatch loop.
///
/// The caller's shell stays where it is for the whole session; the agent
/// child runs with cwd = worktree. path_file is written (single line,
/// worktree path) only when the worktree is preserved, so the shell wrapper
/// cds into it for manual follow-up. All human-resolved outcomes exit 0 —
/// the 10-13 exit-code contract is headless-only.
pub(super) fn run_session(
    created: &CreatedWorktree,
    command: &[String],
    strategy: MergeStrategy,
    skip_hooks: bool,
    config: &Config,
    path_file: Option<&Path>,
) -> Result<()> {
    shield_ctrl_c();

    let agent_env = process::HookEnv {
        main_repo: &created.repo_root,
        worktree: &created.path,
        branch: &created.branch,
        base_branch: &created.base_branch,
    };

    let mut reopen_count: u32 = 0;
    loop {
        if reopen_count > 0 {
            eprintln!("[wt] Reopen #{reopen_count}");
        }
        eprintln!("Entering interactive session: {}", command.join(" "));
        eprintln!("Worktree: {} ({})", created.branch, created.path.display());
        eprintln!("---");

        let status = process::run_agent(command, &created.path, &agent_env, false)
            .map_err(|e| Error::Other(format!("failed to run agent: {e}")))?;
        if !status.success() {
            // Crash / Ctrl+C / non-zero exit: still check worktree state so
            // the user can choose merge / reopen / preserve instead of being
            // silently stranded with a half-made worktree.
            eprintln!(
                "[wt] Agent exited with status {}; checking worktree state...",
                status.code().unwrap_or(-1)
            );
        }

        let state = gather_state(created, config);

        match determine_action(state.uncommitted > 0, state.commits > 0)? {
            Action::Reopen => {
                reopen_count += 1;
                continue;
            }
            Action::CleanupNoChanges => {
                eprintln!("No changes detected. Cleaning up...");
                cleanup_worktree(&created.path, &created.branch, config)?;
                return Ok(());
            }
            Action::ExitPreserve => return preserve(created, path_file),
            Action::MergeAndCleanup => {
                if !state.base_exists {
                    eprintln!(
                        "Base branch '{}' no longer exists. Worktree preserved.",
                        created.base_branch
                    );
                    eprintln!("Resolve manually with: wt merge --into <branch>");
                    return preserve(created, path_file);
                }
                eprintln!("Merging {} into {}...", created.branch, state.merge_target);
                return match merge_pipeline(
                    created,
                    &state.merge_target,
                    strategy,
                    skip_hooks,
                    config,
                ) {
                    MergeOutcome::Merged { .. } => Ok(()),
                    MergeOutcome::Conflict => {
                        eprintln!("Resolve in the worktree: wt sync && wt merge");
                        preserve(created, path_file)
                    }
                    MergeOutcome::NothingToMerge
                    | MergeOutcome::HookFailed
                    | MergeOutcome::Failed => preserve(created, path_file),
                };
            }
        }
    }
}

/// Decision table: state (+ user choice when ambiguous) → action.
fn determine_action(has_uncommitted: bool, has_commits_ahead: bool) -> Result<Action> {
    // No changes at all → cleanup
    if !has_uncommitted && !has_commits_ahead {
        return Ok(Action::CleanupNoChanges);
    }

    // Only committed changes → prompt merge or exit
    if !has_uncommitted && has_commits_ahead {
        return match prompt::merge_prompt() {
            Ok(MergeChoice::Merge) => Ok(Action::MergeAndCleanup),
            Ok(MergeChoice::Exit) | Err(_) => Ok(Action::ExitPreserve),
        };
    }

    // Has uncommitted changes → prompt reopen or exit
    match prompt::exit_prompt() {
        Ok(ExitChoice::Reopen) => Ok(Action::Reopen),
        Ok(ExitChoice::Exit) | Err(_) => Ok(Action::ExitPreserve),
    }
}

/// Keep the worktree; cd the caller's shell into it when a wrapper is
/// present so manual follow-up starts in the right place.
fn preserve(created: &CreatedWorktree, path_file: Option<&Path>) -> Result<()> {
    eprintln!();
    eprintln!("Exiting interactive session. Worktree preserved.");
    eprintln!();
    eprintln!("Your changes are safe. To continue later:");
    eprintln!("  git add . && git commit -m 'your message'");
    eprintln!("  wt merge    # merge and cleanup");
    eprintln!();
    if path_file.is_some() {
        write_path_file(path_file, &created.path)?;
    } else {
        eprintln!("Worktree path: {}", created.path.display());
    }
    Ok(())
}

/// Shield wt from Ctrl+C while the agent runs, so wt survives to show the
/// dispatch prompt. Headless stays unshielded: SIGINT killing the whole
/// process group is the CI-cancel semantic.
///
/// The handler must be *caught*, never SIG_IGN: an ignored disposition is
/// inherited across exec (the agent would become unkillable by Ctrl+C),
/// while a caught one resets to SIG_DFL in the child. ctrlc registers a
/// caught handler on Unix and a process-local SetConsoleCtrlHandler on
/// Windows — exactly those semantics.
fn shield_ctrl_c() {
    if let Err(e) = ctrlc::set_handler(|| {}) {
        eprintln!("Warning: failed to install Ctrl+C shield: {e}");
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Decision table with injected choices (no prompt).
    fn determine_with_choice(
        has_uncommitted: bool,
        has_commits_ahead: bool,
        exit_choice: Option<ExitChoice>,
        merge_choice: Option<MergeChoice>,
    ) -> Action {
        if !has_uncommitted && !has_commits_ahead {
            return Action::CleanupNoChanges;
        }
        if !has_uncommitted && has_commits_ahead {
            return match merge_choice {
                Some(MergeChoice::Merge) => Action::MergeAndCleanup,
                Some(MergeChoice::Exit) | None => Action::ExitPreserve,
            };
        }
        match exit_choice {
            Some(ExitChoice::Reopen) => Action::Reopen,
            Some(ExitChoice::Exit) | None => Action::ExitPreserve,
        }
    }

    #[test]
    fn test_determine_no_changes() {
        let action = determine_with_choice(false, false, Some(ExitChoice::Exit), None);
        assert_eq!(action, Action::CleanupNoChanges);
    }

    #[test]
    fn test_determine_only_commits_ahead_merge() {
        let action = determine_with_choice(false, true, None, Some(MergeChoice::Merge));
        assert_eq!(action, Action::MergeAndCleanup);
    }

    #[test]
    fn test_determine_only_commits_ahead_exit() {
        let action = determine_with_choice(false, true, None, Some(MergeChoice::Exit));
        assert_eq!(action, Action::ExitPreserve);
    }

    #[test]
    fn test_determine_only_commits_ahead_no_choice_defaults_to_exit() {
        let action = determine_with_choice(false, true, None, None);
        assert_eq!(action, Action::ExitPreserve);
    }

    #[test]
    fn test_determine_uncommitted_reopen() {
        let action = determine_with_choice(true, false, Some(ExitChoice::Reopen), None);
        assert_eq!(action, Action::Reopen);
    }

    #[test]
    fn test_determine_uncommitted_exit() {
        let action = determine_with_choice(true, true, Some(ExitChoice::Exit), None);
        assert_eq!(action, Action::ExitPreserve);
    }

    #[test]
    fn test_determine_uncommitted_none_defaults_to_exit() {
        let action = determine_with_choice(true, false, None, None);
        assert_eq!(action, Action::ExitPreserve);
    }
}
