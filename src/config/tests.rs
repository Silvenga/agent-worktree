use super::*;

#[test]
fn test_global_config_defaults() {
    let config = GlobalConfig::default();
    assert_eq!(config.general.merge_strategy, MergeStrategy::Merge);
    assert_eq!(config.general.sync_strategy, SyncStrategy::Merge);
    assert!(config.general.copy_files.is_empty());
    assert!(config.hooks.post_create.is_empty());
}

#[test]
fn test_project_config_defaults() {
    let config = ProjectConfig::default();
    assert!(config.general.trunk.is_none());
    assert!(config.general.merge_strategy.is_none());
    assert!(config.general.sync_strategy.is_none());
    assert!(config.general.copy_files.is_empty());
}

#[test]
fn test_global_config_parse() {
    let toml = r#"
[general]
merge_strategy = "merge"
copy_files = ["*.secret"]

[hooks]
post_create = ["npm install"]
pre_merge = ["npm test"]
"#;
    let config: GlobalConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.general.merge_strategy, MergeStrategy::Merge);
    assert_eq!(config.general.copy_files, vec!["*.secret"]);
    assert_eq!(config.hooks.post_create, vec!["npm install"]);
    assert_eq!(config.hooks.pre_merge, vec!["npm test"]);
}

#[test]
fn test_project_config_parse() {
    let toml = r#"
[general]
trunk = "develop"
copy_files = [".env", ".env.local"]

[hooks]
post_create = ["pnpm install"]
"#;
    let config: ProjectConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.general.trunk, Some("develop".to_string()));
    assert_eq!(config.general.copy_files, vec![".env", ".env.local"]);
    assert_eq!(config.hooks.post_create, vec!["pnpm install"]);
}

#[test]
fn test_merge_hooks_empty_project() {
    let global = vec!["global-hook".to_string()];
    let project: Vec<String> = vec![];
    let merged = merge_hooks(&global, &project);
    assert_eq!(merged, vec!["global-hook"]);
}

#[test]
fn test_merge_hooks_project_overrides() {
    let global = vec!["global-hook".to_string()];
    let project = vec!["project-hook".to_string()];
    let merged = merge_hooks(&global, &project);
    assert_eq!(merged, vec!["project-hook"]);
}

#[test]
fn test_merge_strategy_roundtrip() {
    // Test via GlobalConfig since toml can't serialize bare enums
    let toml_squash = r#"[general]
merge_strategy = "squash"
"#;
    let config: GlobalConfig = toml::from_str(toml_squash).unwrap();
    assert_eq!(config.general.merge_strategy, MergeStrategy::Squash);
}

// =========================================================================
// Additional tests for better coverage
// =========================================================================

#[test]
fn test_merge_strategy_merge() {
    let toml = r#"[general]
merge_strategy = "merge"
"#;
    let config: GlobalConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.general.merge_strategy, MergeStrategy::Merge);
}

#[test]
fn test_sync_strategy_defaults() {
    assert_eq!(SyncStrategy::default(), SyncStrategy::Merge);
}

#[test]
fn test_sync_strategy_parse() {
    #[derive(Deserialize)]
    struct TestConfig {
        sync: SyncStrategy,
    }
    let toml = r#"sync = "merge""#;
    let config: TestConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.sync, SyncStrategy::Merge);

    let toml = r#"sync = "rebase""#;
    let config: TestConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.sync, SyncStrategy::Rebase);
}

#[test]
fn test_hooks_config_defaults() {
    let hooks = HooksConfig::default();
    assert!(hooks.post_create.is_empty());
    assert!(hooks.pre_merge.is_empty());
    assert!(hooks.post_merge.is_empty());
}

#[test]
fn test_hooks_config_with_post_merge() {
    let toml = r#"
[hooks]
post_merge = ["git push", "notify-team"]
"#;
    let config: GlobalConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.hooks.post_merge, vec!["git push", "notify-team"]);
}

#[test]
fn test_project_general_config_defaults() {
    let general = ProjectGeneralConfig::default();
    assert!(general.trunk.is_none());
    assert!(general.merge_strategy.is_none());
    assert!(general.sync_strategy.is_none());
    assert!(general.copy_files.is_empty());
}

#[test]
fn test_config_base_dir() {
    let result = Config::base_dir();
    assert!(result.is_ok());
    let path = result.unwrap();
    assert!(path.to_string_lossy().contains(".agent-worktree"));
}

#[test]
fn test_resolve_base_dir_with_env() {
    let path = Config::resolve_base_dir(Some("/tmp/custom-wt")).unwrap();
    assert_eq!(path, PathBuf::from("/tmp/custom-wt"));
}

#[test]
fn test_resolve_base_dir_empty_env_falls_back() {
    let path = Config::resolve_base_dir(Some("")).unwrap();
    assert!(path.to_string_lossy().contains(".agent-worktree"));
}

#[test]
fn test_resolve_base_dir_none_falls_back() {
    let path = Config::resolve_base_dir(None).unwrap();
    assert!(path.to_string_lossy().contains(".agent-worktree"));
}

#[test]
fn test_error_display() {
    let err = Error::NoHome;
    assert_eq!(err.to_string(), "home directory not found");
}

#[test]
fn test_global_config_serialize() {
    let config = GlobalConfig {
        general: GeneralConfig {
            merge_strategy: MergeStrategy::Merge,
            sync_strategy: SyncStrategy::default(),
            copy_files: vec![".env".to_string()],
            submodules: true,
            submodule_jobs: 8,
        },
        hooks: HooksConfig {
            post_create: vec!["npm install".to_string()],
            pre_merge: vec![],
            post_merge: vec![],
        },
    };
    let serialized = toml::to_string(&config).unwrap();
    assert!(serialized.contains("merge"));
    assert!(serialized.contains(".env"));
    assert!(serialized.contains("npm install"));
}

#[test]
fn test_project_merge_strategy_override() {
    let toml = r#"
[general]
merge_strategy = "merge"
"#;
    let config: ProjectConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.general.merge_strategy, Some(MergeStrategy::Merge));
}

#[test]
fn test_project_merge_strategy_absent() {
    let toml = r#"
[general]
trunk = "develop"
"#;
    let config: ProjectConfig = toml::from_str(toml).unwrap();
    assert!(config.general.merge_strategy.is_none());
}

#[test]
fn test_global_config_parse_sync_strategy() {
    let toml = r#"
[general]
sync_strategy = "merge"
"#;
    let config: GlobalConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.general.sync_strategy, SyncStrategy::Merge);
}

#[test]
fn test_project_sync_strategy_override() {
    let toml = r#"
[general]
sync_strategy = "merge"
"#;
    let config: ProjectConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.general.sync_strategy, Some(SyncStrategy::Merge));
}

#[test]
fn test_project_sync_strategy_absent() {
    let toml = r#"
[general]
trunk = "develop"
"#;
    let config: ProjectConfig = toml::from_str(toml).unwrap();
    assert!(config.general.sync_strategy.is_none());
}

#[test]
fn test_project_config_serialize() {
    let config = ProjectConfig {
        general: ProjectGeneralConfig {
            trunk: Some("develop".to_string()),
            merge_strategy: None,
            sync_strategy: None,
            submodules: None,
            submodule_jobs: None,
            copy_files: vec![".env.local".to_string()],
        },
        hooks: HooksConfig::default(),
    };
    let serialized = toml::to_string(&config).unwrap();
    assert!(serialized.contains("develop"));
    assert!(serialized.contains(".env.local"));
}

#[test]
fn test_submodules_default_true() {
    assert!(GeneralConfig::default().submodules);
    // Missing key in TOML must also resolve to true (serde default).
    let config: GlobalConfig = toml::from_str("").unwrap();
    assert!(config.general.submodules);
}

#[test]
fn test_submodules_global_off() {
    let toml = r#"
[general]
submodules = false
"#;
    let config: GlobalConfig = toml::from_str(toml).unwrap();
    assert!(!config.general.submodules);
}

#[test]
fn test_submodules_project_override() {
    let toml = r#"
[general]
submodules = false
"#;
    let config: ProjectConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.general.submodules, Some(false));

    let config: ProjectConfig = toml::from_str("").unwrap();
    assert!(config.general.submodules.is_none());
}

#[test]
fn test_submodule_jobs_default_is_eight() {
    assert_eq!(GeneralConfig::default().submodule_jobs, 8);
    // Missing key in TOML must also resolve to 8 (serde default).
    let config: GlobalConfig = toml::from_str("").unwrap();
    assert_eq!(config.general.submodule_jobs, 8);
}

#[test]
fn test_submodule_jobs_global_override() {
    let toml = r#"
[general]
submodule_jobs = 1
"#;
    let config: GlobalConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.general.submodule_jobs, 1);
}

#[test]
fn test_submodule_jobs_project_override() {
    let toml = r#"
[general]
submodule_jobs = 16
"#;
    let config: ProjectConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.general.submodule_jobs, Some(16));

    let config: ProjectConfig = toml::from_str("").unwrap();
    assert!(config.general.submodule_jobs.is_none());
}

#[test]
fn test_merge_hooks_both_empty() {
    let global: Vec<String> = vec![];
    let project: Vec<String> = vec![];
    let merged = merge_hooks(&global, &project);
    assert!(merged.is_empty());
}

#[test]
fn test_global_config_clone() {
    let config = GlobalConfig::default();
    let cloned = config.clone();
    assert_eq!(cloned.general.merge_strategy, config.general.merge_strategy);
}

#[test]
fn test_merge_strategy_equality() {
    assert_eq!(MergeStrategy::Squash, MergeStrategy::Squash);
    assert_ne!(MergeStrategy::Squash, MergeStrategy::Merge);
}

#[test]
fn test_sync_strategy_equality() {
    assert_eq!(SyncStrategy::Rebase, SyncStrategy::Rebase);
    assert_ne!(SyncStrategy::Rebase, SyncStrategy::Merge);
}

#[test]
fn test_merge_strategy_copy() {
    let strategy = MergeStrategy::Squash;
    let copied = strategy;
    assert_eq!(strategy, copied);
}

#[test]
fn test_config_debug() {
    let config = GlobalConfig::default();
    let debug = format!("{:?}", config);
    assert!(debug.contains("GlobalConfig"));
}
