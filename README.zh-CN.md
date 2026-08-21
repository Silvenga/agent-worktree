# agent-worktree

[![npm version](https://img.shields.io/npm/v/agent-worktree)](https://www.npmjs.com/package/agent-worktree)

为 AI 编程 agent 设计的 Git worktree 工作流工具。提供隔离的并行开发环境。

[English](README.md)

![Cover](cover.jpg)

## 为什么需要

AI 编程 agent 在隔离环境中工作效果最佳：

- **并行执行**：同时运行多个 agent，互不干扰
- **环境隔离**：每个功能独立工作目录
- **Run 引擎**："即用即删"工作流 — 创建 worktree、运行 agent、合并、清理。交互式（`wt run -i`）给人用，headless（`wt run`）给 CI/编排器用

## 安装

```bash
npm install -g agent-worktree
```

更新到最新版本：

```bash
wt update
```

> **Windows 注意** — `wt update` 会重装 npm 包，运行中的 `wt.exe` 被 OS
> 锁定时会失败。更新前先关闭所有运行 `wt` 的 shell。

Shell 集成会自动安装。如需手动重新安装：

```bash
wt setup
```

支持的 shell：bash、zsh、fish、PowerShell

## 快速开始

```bash
# 创建 worktree 并进入
wt new feature-x

# ... 开发、提交 ...

# 合并（默认 merge 回创建时所在的分支）
wt merge            # 保留 worktree
wt merge -d         # 合并后删除 worktree
```

其他常用命令：

```bash
wt ls              # 列出所有 worktree（含 BASE 分支信息）
wt cd feature-y    # 切换到另一个 worktree
wt cd              # 返回主仓库
```

## 运行 Agent（`wt run`）

单一引擎：创建 worktree → 在其中运行 agent（注入 `WT_*` 环境变量）→ 按
结果分派。Agent 命令写在 `--` 之后，直接执行、不经 shell——没有引号坑。
失败一律保留 worktree 现场。

### 交互式（`-i`）

agent 退出后由你决定去留：

```bash
wt run -i -- claude
wt run fix-bug -i -- codex --some-flag
```

Agent 退出时（含 crash / Ctrl+C——信号只作用于 agent），`wt` 检查状态：

- **无改动** → 自动清理，无 prompt
- **只有 commits** → `[m]` 合并到 base 分支 / `[q]` 保留 worktree
- **有未提交更改** → `[r]` 重开 agent / `[q]` 保留 worktree

保留时 shell 切进该 worktree 方便善后（`git commit` 后 `wt merge`）。
决定即退出码 0；在已有 worktree 内启动会被拒绝。

### Headless（默认）

策略事前声明——面向 CI / 编排器 / ralph 循环，无需 shell 集成：

```bash
wt run fix-bug -- claude -p "fix the bug" --dangerously-skip-permissions
wt run --on-success merge --json -- codex exec "add tests"
```

| Agent 结果 | 行为 | 退出码 |
|---|---|---|
| 非零退出 | 保留 worktree | 10 |
| 无改动 | 自动清理 | 0 |
| 遗留未提交改动 | 保留 worktree（agent 应自行 commit） | 11 |
| 有 commits + `--on-success keep`（默认） | 保留 worktree | 0 |
| 有 commits + `--on-success merge` | pre-merge hooks → 原子合并 → 清理 | 0 |
| pre/post-merge hook 失败 | 保留 worktree | 12 |
| 合并冲突或合并失败 | 保留 worktree，主仓库 HEAD 复位 | 13 |

`--json` 向 stdout 输出单个结果对象（agent 的 stdout 转投 stderr）。其他
flag：`--base <branch>`、`-s <strategy>`（squash/merge）、`-H`（跳过
pre-merge hooks）。`--on-success` 与 `--json` 为 headless 专属，与 `-i` 互斥。

## 命令

### Worktree 管理

| 命令 | 描述 |
|------|------|
| `wt new [branch]` | 从当前分支创建 worktree（省略则随机命名） |
| `wt new --base <branch>` | 指定 base 分支（默认为当前分支） |
| `wt cd [branch]` | 切换到 worktree（省略则返回主仓库） |
| `wt ls` | 列出 worktree |
| `wt ls -l` | 显示每个 worktree 的完整路径 |
| `wt ls --json` | 机器可读 JSON 输出（供 dashboard/脚本消费） |
| `wt mv <old> <new>` | 重命名 worktree（`.` 表示当前） |
| `wt rm <branch>` | 删除 worktree（`.` 表示当前） |
| `wt rm -f <branch>` | 强制删除（含未提交更改） |
| `wt clean` | 清理与各自 base 分支（fallback trunk）无差异的 worktree；脏 worktree 跳过 |
| `wt clean --dry-run` | 预览将被清理的 worktree（不实际删除） |

### 工作流

| 命令 | 描述 |
|------|------|
| `wt merge` | 合并到 base 分支（fallback trunk，默认 merge） |
| `wt merge -s <strategy>` | 指定合并策略（squash/merge） |
| `wt merge --into <branch>` | 合并到指定分支（覆盖 base） |
| `wt merge -d` | 合并后删除 worktree（默认保留） |
| `wt merge -H` | 跳过 pre-merge hooks |
| `wt sync` | 从 base 分支同步更新（fallback trunk，默认 merge） |
| `wt sync -s <strategy>` | 指定同步策略（rebase/merge） |
| `wt sync --from <branch>` | 从指定分支同步（覆盖 base） |
| `wt sync --continue` | 解决冲突后继续 |
| `wt sync --abort` | 放弃同步 |
| `wt run [branch] -- <cmd>` | Headless：建 worktree → 跑 agent → keep/merge |
| `wt run [branch] -i -- <cmd>` | 交互式：同一流程，agent 退出后 prompt |

### 信息

| 命令 | 描述 |
|------|------|
| `wt status` | 显示当前 worktree 信息（含 `wt sync` 进行中的 rebase/merge 状态及恢复指引） |
| `wt status --json` | 机器可读 JSON 输出 |
| `wt update` | 更新到最新版本 |

### 配置

| 命令 | 描述 |
|------|------|
| `wt setup` | 安装 shell 集成（自动检测） |
| `wt setup --shell zsh` | 为指定 shell 安装 |
| `wt init` | 初始化项目配置 |
| `wt init --trunk <branch>` | 初始化并指定 trunk 分支 |
| `wt init --merge-strategy <strategy>` | 设置默认合并策略（squash/merge） |
| `wt init --sync-strategy <strategy>` | 设置默认同步策略（rebase/merge） |
| `wt init --copy-files <pattern>` | 指定要复制到新 worktree 的文件（可重复） |

## 配置文件

### 基础目录

默认 `~/.agent-worktree`。通过 `AGENT_WORKTREE_DIR` 覆盖：

```bash
export AGENT_WORKTREE_DIR=/data/agent-worktree
```

### 全局配置 `$AGENT_WORKTREE_DIR/config.toml`（默认 `~/.agent-worktree/config.toml`）

```toml
[general]
merge_strategy = "merge"   # squash | merge
sync_strategy = "merge"    # rebase | merge
copy_files = [".env", ".env.*"]  # gitignore 风格的文件模式
submodules = true          # 新 worktree 自动 init submodule
submodule_jobs = 8         # submodule init 的并行 clone 数

[hooks]
post_create = ["pnpm install"]
pre_merge = ["pnpm test", "pnpm lint"]
post_merge = []
```

> **`copy_files`** — gitignore 风格 pattern，且必须停留在 repo 内：
> 拒绝 `/` 开头和 `..` 段。符号链接条目一律跳过。文件系统支持时走写时复制，
> 故 `copy_files = ["node_modules"]` 在这类文件系统上近乎零成本。
>
> **Hook 信任边界** — hooks 通过 `sh -c`（Windows `cmd /C`）执行，
> 无沙箱无超时。把 `.agent-worktree.toml` 当 committed shell script
> 对待：只跑你愿意 `bash` 的 repo 的 hooks。
>
> **Hook CWD** — 所有 hook 一律以 worktree 根为工作目录。
>
> **Submodule** — `git worktree add` 会留下空的 submodule 目录，故存在
> `.gitmodules` 时新 worktree 自动执行 `git submodule update --init --recursive`。
> git 把 submodule gitdir 存在 per-worktree 目录，主仓不复用：每个 worktree
> 都要联网重新 clone 全部 submodule，嵌套树需以分钟计。`submodule_jobs`
> （默认 `8`）控制并行 clone 数；设 `submodules = false` 可跳过，改为手动 init。
>
> **`core.hooksPath`** — 相对路径在 worktree 内无法解析，创建时检测到会打印警告。
>
> **Hook 环境变量** — 每个 hook 都注入以下变量，脚本可引用路径而不硬编码：
>
> | 变量 | 值 |
> | --- | --- |
> | `WT_MAIN_REPO` | 主仓库根 |
> | `WT_WORKTREE` | 新 worktree 绝对路径 |
> | `WT_BRANCH` | worktree 分支名 |
> | `WT_BASE_BRANCH` | base 分支（`new` 为创建来源，`merge` 为合并目标） |
>
> 不支持写时复制的文件系统上，重目录用软链优于 `copy_files`：
>
> ```toml
> [hooks]
> post_create = ['ln -s "$WT_MAIN_REPO/node_modules" node_modules']
> ```

### 项目配置 `.agent-worktree.toml`

项目配置覆盖全局。`trunk` 仅存在于项目配置，其他字段合并生效。

```toml
[general]
trunk = "main"  # trunk 分支（省略则自动检测）
merge_strategy = "merge"  # 覆盖全局合并策略
sync_strategy = "merge"   # 覆盖全局同步策略
submodules = false        # 覆盖全局 submodule 自动 init
submodule_jobs = 16       # 覆盖全局 submodule 并行 clone 数
copy_files = ["*.secret.*"]  # 追加到全局 copy_files

[hooks]
post_create = ["pnpm install"]  # 非空时覆盖全局同名 hook
```

## 存储结构

```
~/.agent-worktree/
├── config.toml                    # 全局配置
└── workspaces/
    └── {project}/
        ├── {branch_name}.toml     # worktree 元数据
        ├── {branch_name}/         # worktree 目录
        └── ...
```

## 许可证

MIT
