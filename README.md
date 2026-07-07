# agent-worktree

[![npm version](https://img.shields.io/npm/v/agent-worktree)](https://www.npmjs.com/package/agent-worktree)

A Git worktree workflow tool for AI coding agents. Enables parallel development with isolated environments.

[中文文档](README.zh-CN.md)

![Cover](cover.jpg)

## Why

AI coding agents work best with isolated environments:

- **Parallel execution**: Run multiple agents simultaneously without interference
- **Clean separation**: Each feature gets its own working directory
- **Run engine**: "Use and discard" workflow — create worktree, run agent, merge, cleanup. Interactive (`wt run -i`) for humans, headless (`wt run`) for CI/orchestrators

## Install

```bash
npm install -g agent-worktree
```

Update to the latest version:

```bash
wt update
```

> **Windows note** — `wt update` reinstalls the npm package, which fails if
> any `wt` process is running because Windows locks the running `.exe`.
> Close all shells running `wt` before updating.

Shell integration is installed automatically. To reinstall manually:

```bash
wt setup
```

Supported shells: bash, zsh, fish, PowerShell

## Quick Start

```bash
# Create a worktree and enter it
wt new feature-x

# ... develop, commit ...

# Merge back (merges to the branch you were on when creating)
wt merge            # keeps worktree
wt merge -d         # deletes worktree after merge
```

Other useful commands:

```bash
wt ls              # List all worktrees (with BASE branch info)
wt cd feature-y    # Switch to another worktree
wt cd              # Return to main repository
```

## Run an Agent (`wt run`)

One engine: create worktree → run agent inside (`WT_*` env vars injected) →
dispatch on the outcome. The agent command comes after `--` and runs
directly, without a shell — no quoting pitfalls. Failures always preserve
the worktree for inspection.

### Interactive (`-i`)

You decide after the agent exits:

```bash
wt run -i -- claude
wt run fix-bug -i -- codex --some-flag
```

After the agent exits (even on crash / Ctrl+C — the signal reaches only the
agent), `wt` checks the worktree:

- **No changes** → cleanup, no prompt
- **Only commits** → `[m]` merge into base branch / `[q]` keep worktree
- **Uncommitted changes** → `[r]` reopen the agent / `[q]` keep worktree

When a worktree is kept, your shell switches into it (`git commit`, then
`wt merge`). Interactive sessions exit 0 once you've decided; starting one
from inside an existing worktree is refused.

### Headless (default)

Policy declared upfront — for CI / orchestrators / ralph-style loops, no
shell integration required:

```bash
wt run fix-bug -- claude -p "fix the bug" --dangerously-skip-permissions
wt run --on-success merge --json -- codex exec "add tests"
```

| Agent result | Behavior | Exit code |
|---|---|---|
| Non-zero exit | Keep worktree | 10 |
| No changes | Auto cleanup | 0 |
| Uncommitted changes left | Keep worktree (agent should commit) | 11 |
| Commits + `--on-success keep` (default) | Keep worktree | 0 |
| Commits + `--on-success merge` | pre-merge hooks → atomic merge → cleanup | 0 |
| pre/post-merge hook failure | Keep worktree | 12 |
| Merge conflict or merge failure | Keep worktree, main repo HEAD restored | 13 |

`--json` prints a single result object to stdout (agent stdout is diverted
to stderr). Other flags: `--base <branch>`, `-s <strategy>` (squash/merge),
`-H` (skip pre-merge hooks). `--on-success`/`--json` are headless-only and
conflict with `-i`.

## Commands

### Worktree Management

| Command | Description |
|---------|-------------|
| `wt new [branch]` | Create worktree from current branch (random name if omitted) |
| `wt new --base <branch>` | Create from specific base branch (default: current branch) |
| `wt cd [branch]` | Switch to worktree (omit branch to return to main repo) |
| `wt ls` | List worktrees |
| `wt ls -l` | Show full path for each worktree |
| `wt ls --json` | Machine-readable JSON output (for dashboards/scripts) |
| `wt mv <old> <new>` | Rename worktree (use `.` for current) |
| `wt rm <branch>` | Remove worktree (use `.` for current) |
| `wt rm -f <branch>` | Force remove with uncommitted changes |
| `wt clean` | Remove worktrees with no diff from their base branch (falls back to trunk); dirty worktrees are skipped |
| `wt clean --dry-run` | Preview which worktrees would be cleaned |

### Workflow

| Command | Description |
|---------|-------------|
| `wt merge` | Merge to base branch (falls back to trunk, default: squash) |
| `wt merge -s <strategy>` | Merge with strategy (squash/merge) |
| `wt merge --into <branch>` | Merge to specific branch (overrides base) |
| `wt merge -d` | Delete worktree after merge (default: keep) |
| `wt merge -H` | Skip pre-merge hooks |
| `wt sync` | Sync from base branch (falls back to trunk, default: rebase) |
| `wt sync -s <strategy>` | Sync with strategy (rebase/merge) |
| `wt sync --from <branch>` | Sync from specific branch (overrides base) |
| `wt sync --continue` | Continue after resolving conflicts |
| `wt sync --abort` | Abort sync |
| `wt run [branch] -- <cmd>` | Headless: create worktree → run agent → keep/merge |
| `wt run [branch] -i -- <cmd>` | Interactive: same flow, prompt after agent exits |

### Info

| Command | Description |
|---------|-------------|
| `wt status` | Show current worktree info (also reports in-progress `wt sync` rebase/merge with recovery hints) |
| `wt status --json` | Machine-readable JSON output |
| `wt update` | Update to the latest version |

### Configuration

| Command | Description |
|---------|-------------|
| `wt setup` | Install shell integration (auto-detect) |
| `wt setup --shell zsh` | Install for specific shell |
| `wt init` | Initialize project config |
| `wt init --trunk <branch>` | Initialize with specific trunk branch |
| `wt init --merge-strategy <strategy>` | Set default merge strategy (squash/merge) |
| `wt init --sync-strategy <strategy>` | Set default sync strategy (rebase/merge) |
| `wt init --copy-files <pattern>` | Files to copy to new worktrees (repeatable) |

## Configuration

### Base Directory

Defaults to `~/.agent-worktree`. Override via `AGENT_WORKTREE_DIR`:

```bash
export AGENT_WORKTREE_DIR=/data/agent-worktree
```

### Global Config `$AGENT_WORKTREE_DIR/config.toml` (default `~/.agent-worktree/config.toml`)

```toml
[general]
merge_strategy = "squash"  # squash | merge
sync_strategy = "rebase"   # rebase | merge
copy_files = [".env", ".env.*"]  # Gitignore-style patterns for files to copy
submodules = true          # Auto-init submodules in new worktrees

[hooks]
post_create = ["pnpm install"]
pre_merge = ["pnpm test", "pnpm lint"]
post_merge = []
```

> **`copy_files` constraints** — patterns are gitignore-style and must stay
> inside the repo: leading `/` (absolute paths) and `..` traversal are
> rejected. Symlink entries are skipped entirely — neither the link nor its
> target is copied (share paths via a `post_create` hook instead). Files are
> copied using Reflink where the filesystem supports it (e.g., BTRFS, ZFS,
> XFS, APFS, ReFS, etc.), falling back to a plain copy otherwise.
>
> **Hook trust boundary** — hooks run via `sh -c` (or `cmd /C` on Windows)
> with no sandboxing or timeout. Treat `.agent-worktree.toml` like any
> committed shell script: only run repos whose hooks you would `bash` directly.
>
> **Hook CWD** — `pre_merge` and `post_merge` always run with the worktree
> root as the working directory. `post_create` runs in the new worktree.
>
> **Submodules & git hooks** — `git worktree add` leaves submodule
> directories empty, so new worktrees auto-run
> `git submodule update --init --recursive` when `.gitmodules` exists
> (disable with `submodules = false`). A relative `core.hooksPath` does not
> resolve inside worktrees; creation prints a warning when one is detected.
>
> **Hook environment** — every hook receives these variables, so scripts can
> reference paths without hardcoding them:
>
> | Variable | Value |
> | --- | --- |
> | `WT_MAIN_REPO` | Main repository root |
> | `WT_WORKTREE` | New worktree's absolute path |
> | `WT_BRANCH` | Worktree's branch name |
> | `WT_BASE_BRANCH` | Base branch (creation source for `new`, merge target for `merge`) |
>
> This makes `post_create` a cheaper alternative to `copy_files` when you only
> want to share — not duplicate — heavy directories like dependencies.
> `copy_files` already uses copy-on-write where the filesystem supports it
> (btrfs/XFS reflink, APFS clonefile, ReFS block cloning), so a plain
> `copy_files = ["node_modules"]` is near-free on those filesystems; the
> symlink trick is mainly useful on filesystems without CoW:
>
> ```toml
> [hooks]
> # Symlink instead of copying: no disk cost, no copy time.
> post_create = ['ln -s "$WT_MAIN_REPO/node_modules" node_modules']
> ```

### Project Config `.agent-worktree.toml`

Project config overrides global. `trunk` is project-only; other fields are merged.

```toml
[general]
trunk = "main"  # Trunk branch (auto-detected if omitted)
merge_strategy = "merge"  # Override global merge strategy
sync_strategy = "merge"   # Override global sync strategy
submodules = false        # Override global submodule auto-init
copy_files = ["*.secret.*"]  # Appended to global copy_files

[hooks]
post_create = ["pnpm install"]  # Replaces global hooks if set
```

## Storage Layout

```
~/.agent-worktree/
├── config.toml                    # Global config
└── workspaces/
    └── {project}/
        ├── {branch_name}.toml     # Worktree metadata
        ├── {branch_name}/         # Worktree directory
        └── ...
```

## License

MIT
