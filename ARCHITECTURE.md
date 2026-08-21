# agent-worktree 架构设计文档

## 概述

agent-worktree 是一个 Git Worktree 工作流工具，为 AI coding agent 提供隔离的并行开发环境。

**核心价值**：
- 并行开发：同时运行多个 agent，互不干扰
- 环境隔离：每个功能独立工作目录
- 流程自动化：`wt run` 引擎（headless / interactive 双分派）实现"即用即删"的完整开发闭环

---

## 目录结构

基础目录默认 `~/.agent-worktree`，可通过 `AGENT_WORKTREE_DIR` 环境变量覆盖（空串视同未设）。

```
$AGENT_WORKTREE_DIR/  (默认 ~/.agent-worktree/)
├── config.toml                    # 全局配置
└── workspaces/                    # 所有 worktree 存储位置
    └── {repo}-{hash}/             # 按项目组织（hash 基于仓库绝对路径，防止同名冲突）
        ├── swift-fox.toml         # worktree 元数据（旧版 .status.toml 仍兼容）
        ├── swift-fox/             # 随机生成的分支名
        ├── fix-auth-bug.toml
        ├── fix-auth-bug/          # 用户指定的分支名
        ├── quiet-moon.toml
        └── quiet-moon/
            └── ...                # 项目文件

项目根目录/
└── .agent-worktree.toml           # 项目级配置（可选）
```

### 元数据格式

```toml
created_at = 2024-01-15T10:30:00Z
base_branch = "main"             # 创建时的源分支（merge/sync 默认目标）
```

> 旧版字段（`base_commit`/`trunk`/`snap_command`）已弃用。读取时若缺 `base_branch` 则回退到旧 `trunk` 字段；其他旧字段静默忽略。

---

## 命令设计

### 1. Worktree 管理

```bash
wt new [branch]              # 创建 worktree 并进入（base = current_branch；detached HEAD 时回退 trunk）
wt new [branch] --base <br>  # 显式指定 base 分支（必须存在，覆盖默认；同时记录到 meta）
wt cd [branch]               # 切换到指定 worktree（省略则回到主仓库）
wt ls [--json]               # 列出 worktree（按创建时间降序；--json 机器可读输出）
wt status [--json]           # 查看当前 worktree 详细信息（--json 机器可读输出）
wt mv <old> <new>            # 重命名 worktree 分支（old 可用 . 表示当前）
wt rm <branch> [-f]          # 删除 worktree（branch 可用 . 表示当前）
wt clean [--dry-run]         # 清理所有与 target 无差异的 worktree（target = base_branch > trunk）
```

### 2. 工作流

```bash
wt merge [options]           # 合并当前 worktree（默认 merge 回 base branch，fallback trunk）
    -s, --strategy <squash|merge>  # 合并策略，默认 merge
    --into <branch>          # 合并到指定分支（覆盖 base branch / trunk，校验存在性）
    -d, --delete             # 合并后删除 worktree（默认保留）
    -H, --skip-hooks         # 跳过 pre-merge hook

wt sync [options]            # 从 base branch 同步更新到当前 worktree（fallback trunk）
    -s, --strategy <rebase|merge>  # 同步策略，默认 merge（可被 sync_strategy 配置覆盖）
    --from <branch>          # 指定同步源分支（覆盖 base branch / trunk，校验存在性）
    --continue               # 解决冲突后继续
    --abort                  # 放弃同步，恢复到冲突前状态

wt run [branch] [options] -- <cmd...>  # Run 引擎：建 worktree → 跑 agent → 按结果分派
    --base <branch>          # 同 wt new --base
    -i, --interactive        # 交互分派：agent 退出后 prompt；与 --on-success/--json 互斥
    --on-success <keep|merge>  # [headless] agent 成功且只有 commits 时的动作，默认 keep
    -s, --strategy <squash|merge>  # merge 时的合并策略，默认读 config
    -H, --skip-hooks         # 跳过 pre-merge hook
    --json                   # [headless] stdout 输出单个结果对象（agent stdout 转投 stderr）
```

### 3. 维护

```bash
wt update                    # 更新到最新版本
```

### 4. 配置

```bash
wt setup                     # 安装 shell 集成（自动检测 shell）
wt setup --shell zsh         # 指定 shell
wt init [options]            # 在当前项目初始化配置
    --trunk <branch>         # 主干分支
    --merge-strategy <squash|merge>  # 默认合并策略
    --sync-strategy <rebase|merge>   # 默认同步策略
    --copy-files <pattern>   # 复制文件模式（可重复）
```

---

## Shell 集成

`wt cd`、`wt new`、`wt rm`、`wt mv`、`wt merge`、`wt clean`、`wt run` 等命令需要改变 shell 工作目录，因此需要 shell wrapper。

运行 `wt setup` 自动安装（npm 安装时会自动执行），会在 shell 配置文件中添加 wrapper 函数。

**支持的 shell**：bash、zsh、fish、powershell

**配置文件位置**：
- bash: `~/.bashrc`
- zsh: `~/.zshrc`
- fish: `~/.config/fish/config.fish`
- powershell: `~/Documents/PowerShell/Microsoft.PowerShell_profile.ps1`

### Wrapper 职责边界

Wrapper 职责唯一：**跑二进制 + path_file 存在则 cd**。禁止在 wrapper 内写业务循环——
snap 循环曾长在三套 shell 脚本里各一份，同步维护，已收编进二进制（见 Run 引擎）。

- path_file 格式唯一：单行目标路径。多行格式已废除
- 0/2/3 snap 退出码协议已废除；wrapper 不解释任何 wt 退出码，只透传

### 集成约束

- **Wrapper 必装才能 cd**：`wt cd` 检测无 `--path-file` 直接报错，提示 `wt setup`——不再静默 noop
- **`wt rm .` 防误操**：cwd 在被删 worktree 内且无 wrapper → 拒绝（避免 dangling cwd）
- **rc 文件 marker 严格配对**：`wt setup` 找到孤立 BEGIN/END 直接报错，不动 rc，避免截断
- **path_file 唯一**：bash/zsh wrapper 用 `mktemp` 而非 `$$`（subshell 中 `$$` 是父 PID，并发会撞）
- **Windows update**：`wt update` 调用 npm，运行中的 `wt.exe` 被 OS 锁定 → 先关闭所有 wt 进程

---

## 分支名生成

1. **用户指定**：`wt new fix-auth-bug` → 使用 `fix-auth-bug`
2. **自动生成**：`wt new` → 生成 `形容词-名词` 格式，如 `swift-fox`

词库内置约 100 个形容词 + 100 个名词。冲突时追加数字后缀（`swift-fox-2`）。

---

## Run 引擎（wt run）

单一引擎：「创建 → spawn agent → 检查状态 → 分派」。分派策略二选一，决策者不同：

- **Policy 分派**（默认，headless）：`--on-success` 事前声明，机器在环。面向 CI / 编排器 / `claude -p` 循环
- **Interactive 分派**（`-i`）：agent 退出后 prompt，人事后决策，reopen 循环进程内完成。即原 snap 模式，shell wrapper 不再参与循环

### 共同设计要点

- **agent 命令 `--` 尾随、直接 exec 不经 shell** —— 消灭引号坑；需要 shell 语义时显式 `sh -c '...'`
- **agent 子进程继承 stdio**（TTY 交互无损，claude 等 TUI 直接可用）、注入 `WT_*` 环境变量（同 hooks）、cwd = worktree
- **复用创建路径**：`lifecycle/new.rs::create_worktree_core()` 为 `wt new` 与 `wt run` 单一实现
- **merge 编排单一 pipeline**：pre_merge hooks → 主仓预检（dirty/in-progress 拒绝）→ dry-run → execute_merge → post_merge hooks → cleanup。headless / interactive 共用一份实现，调用方只映射结果（退出码 vs prompt 文案）。禁止按分派策略复制编排
- **失败一律保留现场**：worktree 不删、主仓库 HEAD 复位（复用原子 merge 语义）
- **验证闸门 = pre_merge hooks**：不设 `--verify`，同一职责禁双机制；`-H` 跳过
- **不做 auto-commit**：agent 遗留未提交改动，headless 视为未完成（退出码 11），interactive 走 reopen prompt

### Policy 分派（headless，默认）

无 prompt、无 wrapper 依赖、不改变调用方 cwd。

**退出码契约**（机器可依赖，从 10 起避开通用 1）：

| 码 | 含义 |
|---|---|
| 0 | 闭环成功（cleanup / kept / merged） |
| 10 | agent 非零退出 |
| 11 | agent 遗留未提交改动 |
| 12 | pre/post-merge hook 失败 |
| 13 | 合并冲突或合并失败（含 base 分支消失） |

**`--json` 结果对象**：stdout 单行 JSON（agent stdout 转投 stderr，stdout 保持纯净）：`version:1, branch, path, base_branch, merge_target, agent_exit_code, commits, uncommitted, outcome, worktree_removed, exit_code`。`outcome` 取值：`no_changes | kept | merged | agent_failed | uncommitted | hook_failed | merge_conflict | merge_failed`。

### Interactive 分派（`-i`）

```bash
wt run -i -- claude                    # 随机分支名
wt run fix-bug -i -- aider --model sonnet  # 指定分支名，参数无需引号
```

agent 退出（含 crash / Ctrl+C / 非零）后检查 git 状态：

| 状态 | 行为 |
|------|------|
| 无改动（uncommitted=❌, commits=❌） | 直接清理 worktree，shell 留原地 |
| 只有 commits（uncommitted=❌, commits=✅） | prompt: [m] merge / [q] exit |
| 有未提交改动（uncommitted=✅） | prompt: [r] reopen / [q] exit |

- **reopen = 进程内重新 spawn agent**，无 shell 参与
- **cwd 语义**：shell 全程留在原地（不再先 cd 进 worktree 再 cd 回）。merge/清理成功 → 不写 path_file；`[q]`/冲突/merge 失败 → path_file = worktree 路径，shell cd 进去（保留「退出留在 worktree」的善后 UX）
- **退出码一律 0**（人已决策，含 preserve）；硬错误（创建失败等）走通用 1。10-13 契约为 headless 专属
- **wrapper 可选**：无 wrapper 仅损失最后的 cd（打印 worktree 路径提示）——不再像旧 snap 硬要求 `wt setup`
- **拒绝在 worktree 内启动**（原 snap 嵌套约束保留）
- **互斥**：`-i` 与 `--on-success`/`--json` clap `conflicts_with`，同一职责禁双机制
- prompt 读 stdin line（非 tty 依赖），EOF/连续无效输入 → Cancelled → 按 preserve 处理

### 信号处理（interactive 专属）

- spawn agent 前注册 **no-op caught SIGINT handler**——禁用 `SIG_IGN`：ignored 处置跨 exec 遗传，agent 将杀不掉；caught handler 在子进程 exec 时自动复位 default
- Windows：`SetConsoleCtrlHandler` 进程本地忽略，等价语义
- 效果：Ctrl+C 只达 agent；agent 死后 wt 存活弹 prompt。git 子进程同理可被 Ctrl+C 杀，wt 走既有失败回滚路径
- headless 不屏蔽：SIGINT 杀全组 = CI cancel 语义，保持现状

### 迁移（0.13 breaking，不向前兼容）

- **`wt new -s/--snap` 与 `snap-continue` 直接删除**，无废弃过渡、无 stub——交互闭环唯一入口 `wt run -i`
- 0/2/3 退出码协议、两行 path_file 格式、`write_path_file_lines` 一并废除
- 旧 wrapper（update 后未重开的 shell 会话）行为：`wt new -s` / `snap-continue` 报 clap 解析错误，不产生破坏性操作；重开 shell 或 `wt setup` 即恢复（npm postinstall 自动刷 wrapper）

### 测试策略

- 决策表纯逻辑单测 + clap 互斥断言 + 旧 snap 表面（`-s`/`snap-continue`）解析报错断言
- Interactive 集成测试走管道 stdin 喂 `m`/`r`/`q`（prompt 是 `read_line`，无 tty 依赖）：覆盖 no_changes / merge / reopen / preserve / 冲突保留全分派表
- 信号：「Ctrl+C 达 agent 不杀 wt」跨平台自动化 ROI 不足，发布前手动验证清单（macOS/Linux/Windows 各一遍），此项须在 PR 描述中留痕

---

## Merge 冲突处理

### 原子 merge（预检测模式）

merge 为原子操作——要么成功，要么 HEAD 回到原分支。不残留中间状态。

```
wt merge
  → 记录主 repo 当前分支为 original
  → checkout target（main repo）
  → dry-run（按真实策略：squash 用 --squash --no-commit，否则 --no-ff）
  → 有冲突？
      YES → 清理 + checkout original → 报错 "先 wt sync 解决冲突"
      NO  → 清理 + 执行真实 merge
              失败 → reset_merge + checkout original → 抛错
              成功 → 跑 post_merge hook → 可选删 worktree
```

### 冲突处理流程

用户需在 worktree 中先 sync 对齐目标分支，再执行 merge：

```bash
wt sync          # 在 worktree 中解决冲突
wt merge         # 无冲突，原子完成
```

### 安全检查与约束

- 主 repo 的未完成 merge / rebase / uncommitted changes → 拒绝
- worktree dirty → 拒绝（消息明示是 worktree 端脏）
- 主 repo dirty → 拒绝（消息明示是 main repo 端脏）
- `--into <branch>` 已被另一 worktree checkout → 拒绝（避免 git 报底层错）
- `MergeStrategy::Merge` already-up-to-date → 返回 "Nothing to merge" 不删 worktree
- 失败一律 rollback HEAD 到原分支 + reset_merge 清 squash 半成品

### merge 入口

- `merge::execute_merge()` 处理 squash/merge 策略，run 引擎（headless/interactive 分派）和 `wt merge` 共用
- `git::dry_run_merge(branch, squash)` 用于预检测冲突，按策略走 `--squash --no-commit` 或 `--no-ff --no-commit`

> 不提供 `wt merge --continue/--abort`：原子语义保证失败 = HEAD 复位，无残留 git 状态需要续/弃。冲突恢复路径只有一条：在 worktree 中 `wt sync`，然后重新 `wt merge`。

---

## Status 输出

`wt status` 显示当前 worktree 的：

- Branch / Base branch（meta）/ Trunk / Merge target（CLI > base_branch（仍存在）> trunk）
- Created at（meta）
- Commits ahead of merge target / Uncommitted count / 累计 diff +ins -del
- Worktree 路径

并检测 git-native 同步状态——`is_rebase_in_progress()` 或 `is_merge_in_progress()` 命中时，追加：

```
State:        REBASE/MERGE IN PROGRESS (sync)
  Resolve conflicts, then: wt sync --continue
  Or abort: wt sync --abort
```

> 仅识别 git-native 状态。`wt merge` 是原子的，不残留可识别状态。

### JSON 输出（`--json`）

`wt ls --json` / `wt status --json` 输出单行 JSON 到 stdout，人类文案一律走 stderr——供 dashboard/statusline/脚本消费。

- Schema 即公开 API：顶层带 `"version": 1`，字段只增不改
- `ls`：`{version, worktrees: [{branch, base_branch, current, uncommitted, commits, insertions, deletions, path, created_at}]}`；无 worktree 时输出空数组（非报错）
- `status`：`{version, branch, base_branch, trunk, merge_target, created_at, commits, uncommitted, insertions, deletions, path, sync_in_progress}`；`sync_in_progress` 取值 `"rebase" | "merge" | null`
- 不含 agent 运行状态（running/waiting）——那属 dashboard 层职责，wt 只吐 git 侧事实

---

## Clean 行为

`wt clean` 遍历当前项目所有 worktree（按 `workspaces_dir/{workspace_id}` 前缀过滤），按以下顺序判定：

1. 跳过 trunk worktree
2. 解析 effective target：`base_branch`（仍存在时）> trunk
3. 与 target 仍有差异 → 跳过
4. uncommitted > 0 → 报告并跳过（`Skipping {branch}: N uncommitted change(s)`）
5. `--dry-run` → 仅打印 "Would clean (no diff from {target})"
6. 真清：`remove_worktree(force=false)` + `delete_branch(force=false)` + 删 meta；如当前 cwd 在被清的 worktree 内，写 path_file 让 shell cd 回主仓库

最终汇总 cleaned/skipped_dirty 计数。

---

## Git 错误处理

`git/mod.rs` 中的 `extract_error()` 统一从命令输出提取错误信息：
- 优先使用 stderr（git 的常规错误输出）
- stderr 为空时 fallback 到 stdout（merge 冲突信息走 stdout）

适用于 `merge`、`commit`、`merge_continue` 等冲突相关命令。

---

## 配置文件

### 全局配置 `$AGENT_WORKTREE_DIR/config.toml`（默认 `~/.agent-worktree/config.toml`）

```toml
[general]
merge_strategy = "merge"                # squash | merge（默认）
sync_strategy = "merge"                 # rebase | merge（默认）
submodules = true                       # 新 worktree 自动 init submodule（默认 true）
submodule_jobs = 8                      # submodule init 并行 clone 数（默认 8）
# 从主仓库复制到新 worktree 的文件（通常是被 gitignore 但开发必需的），支持 glob
copy_files = ["*.secret.*"]

[hooks]
post_create = []
pre_merge = []
post_merge = []
```

### 配置合并规则

- `copy_files`：global + project **追加**合并
- `hooks`：project 非空时**完全替代** global（不追加）
- `merge_strategy` / `sync_strategy` / `submodules` / `submodule_jobs`：project 非空时**覆盖** global（`Option` 语义）
- `trunk`：仅 project 级别配置

### 项目配置 `.agent-worktree.toml`

```toml
[general]
trunk = "main"                    # 主干分支，默认自动检测
merge_strategy = "merge"          # 可选，覆盖全局策略
sync_strategy = "merge"           # 可选，覆盖全局同步策略
copy_files = [".env", ".env.*"]

[hooks]
post_create = ["pnpm install"]
pre_merge = ["pnpm test", "pnpm lint"]
```

### 配置约束与信任边界

- **路径解析**：项目配置从 `git rev-parse --git-common-dir` 上溯到主 repo 根读取——worktree/子目录任意位置行为一致
- **`copy_files` 路径沙箱**：拒绝 `/` 开头（绝对路径）和 `..` 段；符号链接条目一律跳过（链接与目标均不复制，跳过时打印提示）——防止 repo 内软链把 repo 外内容实体化进 worktree。复制优先使用写时复制（reflink/clonefile/DUPLICATE_EXTENTS），不支持时回退为普通拷贝
- **hooks 安全**：hooks 通过 `sh -c`（Windows `cmd /C`）执行，无沙箱无超时——按"committed shell script"信任处理，禁运行不信任 repo
- **hook CWD**：`pre_merge`/`post_merge` 一律 worktree 根；`post_create` 在新 worktree 内
- **hook 环境变量**：所有 hook 注入 `WT_MAIN_REPO`（主仓库根）/`WT_WORKTREE`（worktree 路径）/`WT_BRANCH`（分支名）/`WT_BASE_BRANCH`（base 分支：new=创建来源，merge=合并目标）；叠加于继承环境。让 hook 可移植引用路径，如 `post_create = ['ln -s "$WT_MAIN_REPO/node_modules" node_modules']` 软链替代 `copy_files` 复制。`copy_files` 已在支持的文件系统上使用写时复制，故该软链技巧主要用于不支持 CoW 的文件系统
- **trunk 检测**：`origin/HEAD` > `main` > `master` > 默认 `"main"`
- **submodule 自动 init**：`git worktree add` 留空 submodule 目录；`.gitmodules` 存在且 `submodules = true`（默认）时创建后执行 `git submodule update --init --recursive --jobs <submodule_jobs>`（默认 8），失败仅警告不中止（离线可选 submodule 不该杀掉创建）
- **submodule 输出禁捕获**：git 把 submodule gitdir 存在 per-worktree 目录（`.git/worktrees/<wt>/modules/`），主仓 `.git/modules/` 不复用——每个新 worktree 全量联网重clone，嵌套仓可跑数十分钟。故 init 必须流式透传 git 输出（stdio 继承），静默即等价于「卡死」；失败信息只报退出码，诊断已在屏幕上
- **hooksPath 警告**：相对路径的 `core.hooksPath` 按各命令 CWD 解析，在 linked worktree 内静默失效——创建时检测并打印警告

---

## 模块划分

技术选型：**Rust**——单二进制、无运行时依赖、跨平台、快速启动。

> 各文件职责见 `FILE_TREE.local.md`（单一真源）。本节仅列顶层组织：
>
> - `src/cli/` — Cli struct + Command 分发；`commands/` 按语义分 `nav/` `lifecycle/` `run/` `sys/` 子模块 + 顶层独立命令
> - `src/cli/commands/run/` — Run 引擎：`mod.rs`（args + 状态收集 + 共享 merge pipeline）/ `headless.rs`（policy 分派 + 退出码 + JSON）/ `interactive.rs`（prompt 分派 + reopen 循环 + 信号屏蔽）——单文件 ≤500 行约束下的拆分
> - `src/git/` — repo / worktree / branch / ops 拆分，`mod.rs` 仅导出
> - `src/meta/` — `{branch}.toml` 元数据（兼容旧 `.status.toml`）+ target resolver
> - `src/shell/` — wrapper 脚本生成与安装；wrapper 无业务循环，path_file 单行契约
> - `src/process/` `src/prompt/` `src/update/` `src/util/` — 进程/交互/版本检查/分支名生成
> - `tests/` — 按命令分文件 + `common/mod.rs` 共享辅助
> - `npm/` — 主包 + 各平台二进制子包（postinstall 自动装 shell wrapper）
> - `scripts/` — 构建与发布脚本