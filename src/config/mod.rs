// ===========================================================================
// config - Configuration Loading & Merging
// ===========================================================================

use std::path::{Path, PathBuf};

use directories::BaseDirs;
use serde::{Deserialize, Serialize};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to read config: {0}")]
    Read(#[from] std::io::Error),

    #[error("failed to parse config: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("failed to serialize config: {0}")]
    Serialize(#[from] toml::ser::Error),

    #[error("home directory not found")]
    NoHome,
}

// ---------------------------------------------------------------------------
// Global Config (~/.agent-worktree/config.toml)
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    #[serde(default)]
    pub general: GeneralConfig,

    #[serde(default)]
    pub hooks: HooksConfig,
}

// ---------------------------------------------------------------------------
// Project Config (.agent-worktree.toml)
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    #[serde(default)]
    pub general: ProjectGeneralConfig,

    #[serde(default)]
    pub hooks: HooksConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default)]
    pub merge_strategy: MergeStrategy,

    #[serde(default)]
    pub sync_strategy: SyncStrategy,

    #[serde(default)]
    pub copy_files: Vec<String>,

    /// Auto-init submodules in new worktrees (`git worktree add` leaves them empty)
    #[serde(default = "default_true")]
    pub submodules: bool,

    /// Parallel clone lanes for submodule init.
    ///
    /// Every new worktree re-clones every submodule over the network (git keeps
    /// submodule gitdirs per-worktree), so deep trees are network-bound. Passed
    /// straight to `git submodule update --jobs`; git validates the value.
    #[serde(default = "default_submodule_jobs")]
    pub submodule_jobs: u32,
}

// Manual impl: derived Default would give submodules=false, diverging from
// the serde default and silently disabling the feature when no config exists.
impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            merge_strategy: MergeStrategy::default(),
            sync_strategy: SyncStrategy::default(),
            copy_files: Vec::new(),
            submodules: true,
            submodule_jobs: default_submodule_jobs(),
        }
    }
}

fn default_true() -> bool {
    true
}

/// 8 lanes: enough to hide per-clone latency on a deep submodule tree without
/// hammering the remote with an unbounded fan-out.
fn default_submodule_jobs() -> u32 {
    8
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ProjectGeneralConfig {
    pub trunk: Option<String>,

    pub merge_strategy: Option<MergeStrategy>,

    pub sync_strategy: Option<SyncStrategy>,

    pub submodules: Option<bool>,

    pub submodule_jobs: Option<u32>,

    #[serde(default)]
    pub copy_files: Vec<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct HooksConfig {
    #[serde(default)]
    pub post_create: Vec<String>,

    #[serde(default)]
    pub pre_merge: Vec<String>,

    #[serde(default)]
    pub post_merge: Vec<String>,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum MergeStrategy {
    Squash,
    #[default]
    Merge,
}

impl MergeStrategy {
    pub fn is_squash(&self) -> bool {
        matches!(self, Self::Squash)
    }
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum SyncStrategy {
    Rebase,
    #[default]
    Merge,
}

// ---------------------------------------------------------------------------
// Merged Config (runtime)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Config {
    pub base_dir: PathBuf,
    pub workspaces_dir: PathBuf,
    pub merge_strategy: MergeStrategy,
    pub sync_strategy: SyncStrategy,
    pub copy_files: Vec<String>,
    pub submodules: bool,
    pub submodule_jobs: u32,
    pub hooks: HooksConfig,
    pub trunk: Option<String>,
}

impl Config {
    /// Load and merge global + project config
    pub fn load() -> Result<Self> {
        let base_dir = Self::base_dir()?;
        // Canonicalize base_dir to resolve the macOS /var -> /private/var symlink
        // and keep it consistent with the canonicalized paths git worktree list returns.
        let base_dir = base_dir.canonicalize().unwrap_or(base_dir);
        let workspaces_dir = base_dir.join("workspaces");

        let global = Self::load_global(&base_dir)?;
        let project = Self::load_project()?;

        // Merge: project overrides global
        let merge_strategy = project
            .general
            .merge_strategy
            .unwrap_or(global.general.merge_strategy);
        let sync_strategy = project
            .general
            .sync_strategy
            .unwrap_or(global.general.sync_strategy);
        let submodules = project
            .general
            .submodules
            .unwrap_or(global.general.submodules);
        let submodule_jobs = project
            .general
            .submodule_jobs
            .unwrap_or(global.general.submodule_jobs);
        let mut copy_files = global.general.copy_files;
        copy_files.extend(project.general.copy_files);

        let hooks = HooksConfig {
            post_create: merge_hooks(&global.hooks.post_create, &project.hooks.post_create),
            pre_merge: merge_hooks(&global.hooks.pre_merge, &project.hooks.pre_merge),
            post_merge: merge_hooks(&global.hooks.post_merge, &project.hooks.post_merge),
        };

        Ok(Self {
            base_dir,
            workspaces_dir,
            merge_strategy,
            sync_strategy,
            copy_files,
            submodules,
            submodule_jobs,
            hooks,
            trunk: project.general.trunk,
        })
    }

    /// Resolve the trunk branch: config > auto-detect > default "main".
    pub fn resolve_trunk(&self) -> String {
        self.trunk
            .clone()
            .unwrap_or_else(|| crate::git::detect_trunk().unwrap_or_else(|_| "main".into()))
    }

    pub fn base_dir() -> Result<PathBuf> {
        Self::resolve_base_dir(std::env::var("AGENT_WORKTREE_DIR").ok().as_deref())
    }

    // Split out so tests can exercise both env and fallback branches
    // without mutating process-global env state (unsafe + racy under parallel tests).
    fn resolve_base_dir(env_override: Option<&str>) -> Result<PathBuf> {
        if let Some(dir) = env_override.filter(|s| !s.is_empty()) {
            return Ok(PathBuf::from(dir));
        }
        let base = BaseDirs::new().ok_or(Error::NoHome)?;
        Ok(base.home_dir().join(".agent-worktree"))
    }

    fn load_global(base_dir: &Path) -> Result<GlobalConfig> {
        let path = base_dir.join("config.toml");
        if !path.exists() {
            return Ok(GlobalConfig::default());
        }
        let content = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&content)?)
    }

    fn load_project() -> Result<ProjectConfig> {
        // Resolve from the main repo root (via git --git-common-dir) so the
        // same `.agent-worktree.toml` applies whether the user is in the main
        // repo, a worktree root, or any subdirectory of either. Reading
        // CWD-relative would silently miss the file inside worktrees.
        // Outside any git repo, fall back to default — non-git commands
        // (setup/update) must still load.
        let path = match crate::git::repo_root() {
            Ok(root) => root.join(".agent-worktree.toml"),
            Err(_) => return Ok(ProjectConfig::default()),
        };
        if !path.exists() {
            return Ok(ProjectConfig::default());
        }
        let content = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&content)?)
    }
}

fn merge_hooks(global: &[String], project: &[String]) -> Vec<String> {
    if project.is_empty() {
        global.to_vec()
    } else {
        project.to_vec()
    }
}

#[cfg(test)]
mod tests;
