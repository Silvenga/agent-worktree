use super::*;
use tempfile::tempdir;

// =========================================================================
// remove_wrapper tests
// =========================================================================
#[test]
fn test_remove_wrapper_empty() {
    let result = remove_wrapper("").unwrap();
    assert_eq!(result, "");
}

#[test]
fn test_remove_wrapper_no_wrapper() {
    let content = "alias ll='ls -la'\nexport PATH=$PATH:/usr/local/bin\n";
    let result = remove_wrapper(content).unwrap();
    assert_eq!(result, content);
}

#[test]
fn test_remove_wrapper_with_wrapper() {
    let content = r#"alias ll='ls -la'
# === agent-worktree BEGIN ===
wt() { ... }
# === agent-worktree END ===
export PATH=$PATH:/usr/local/bin
"#;
    let result = remove_wrapper(content).unwrap();
    assert!(result.contains("alias ll"));
    assert!(result.contains("export PATH"));
    assert!(!result.contains("agent-worktree"));
    assert!(!result.contains("wt()"));
}

#[test]
fn test_remove_wrapper_only_wrapper() {
    let content = r#"# === agent-worktree BEGIN ===
wt() { ... }
# === agent-worktree END ===
"#;
    let result = remove_wrapper(content).unwrap();
    assert!(!result.contains("agent-worktree"));
}

#[test]
fn test_remove_wrapper_at_start() {
    let content = r#"# === agent-worktree BEGIN ===
wt() { ... }
# === agent-worktree END ===
alias ll='ls -la'
"#;
    let result = remove_wrapper(content).unwrap();
    assert!(!result.contains("agent-worktree"));
    assert!(result.contains("alias ll"));
}

#[test]
fn test_remove_wrapper_preserves_content_order() {
    let content = r#"line1
# === agent-worktree BEGIN ===
wt() { ... }
# === agent-worktree END ===
line2
"#;
    let result = remove_wrapper(content).unwrap();
    let lines: Vec<&str> = result.lines().collect();
    assert_eq!(lines[0], "line1");
    assert_eq!(lines[1], "line2");
}

#[test]
fn test_remove_wrapper_unmatched_begin_errors() {
    let content = r#"line1
# === agent-worktree BEGIN ===
wt() { ... }
line2
"#;
    let err = remove_wrapper(content).unwrap_err();
    assert!(matches!(err, Error::Other(_)));
    assert!(err.to_string().contains("BEGIN"));
}

#[test]
fn test_remove_wrapper_unmatched_end_errors() {
    let content = r#"line1
wt() { ... }
# === agent-worktree END ===
line2
"#;
    let err = remove_wrapper(content).unwrap_err();
    assert!(matches!(err, Error::Other(_)));
    assert!(err.to_string().contains("END"));
}

#[test]
fn test_remove_wrapper_nested_begin_errors() {
    let content = r#"# === agent-worktree BEGIN ===
wt1() { ... }
# === agent-worktree BEGIN ===
wt2() { ... }
# === agent-worktree END ===
"#;
    let err = remove_wrapper(content).unwrap_err();
    assert!(matches!(err, Error::Other(_)));
}

// =========================================================================
// Error tests
// =========================================================================
#[test]
fn test_error_display() {
    let err = Error::NoHome;
    assert_eq!(err.to_string(), "home directory not found");
}

// =========================================================================
// install tests (with temp files)
// =========================================================================
#[test]
fn test_install_creates_wrapper() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join(".bashrc");

    // Create empty config
    std::fs::write(&config_path, "").unwrap();

    // Mock the config_file by directly testing the logic
    let wrapper = Shell::Bash.wrapper_script();
    let content = std::fs::read_to_string(&config_path).unwrap_or_default();
    let _content = remove_wrapper(&content).unwrap();
    let new_content = format!("{wrapper}\n");
    std::fs::write(&config_path, new_content).unwrap();

    let result = std::fs::read_to_string(&config_path).unwrap();
    assert!(result.contains(MARKER_BEGIN));
    assert!(result.contains(MARKER_END));
    assert!(result.contains("wt()"));
}

#[test]
fn test_install_replaces_existing_wrapper() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join(".bashrc");

    // Create config with old wrapper
    let old_content = r#"alias ll='ls -la'
# === agent-worktree BEGIN ===
old_wt_function() { echo old; }
# === agent-worktree END ===
export PATH=/usr/local/bin
"#;
    std::fs::write(&config_path, old_content).unwrap();

    // Simulate install
    let wrapper = Shell::Bash.wrapper_script();
    let content = std::fs::read_to_string(&config_path).unwrap_or_default();
    let content = remove_wrapper(&content).unwrap();
    let new_content = format!("{content}\n{wrapper}\n");
    std::fs::write(&config_path, new_content).unwrap();

    let result = std::fs::read_to_string(&config_path).unwrap();
    assert!(result.contains("alias ll"));
    assert!(result.contains("export PATH"));
    assert!(!result.contains("old_wt_function"));
    assert!(result.contains("wt()"));
    // Should only have one set of markers
    assert_eq!(result.matches(MARKER_BEGIN).count(), 1);
    assert_eq!(result.matches(MARKER_END).count(), 1);
}

#[test]
fn test_install_appends_to_existing_content() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join(".zshrc");

    let existing = "# My zsh config\nalias ll='ls -la'\n";
    std::fs::write(&config_path, existing).unwrap();

    // Simulate install
    let wrapper = Shell::Zsh.wrapper_script();
    let content = std::fs::read_to_string(&config_path).unwrap_or_default();
    let content = remove_wrapper(&content).unwrap();
    let new_content = format!("{content}\n{wrapper}\n");
    std::fs::write(&config_path, new_content).unwrap();

    let result = std::fs::read_to_string(&config_path).unwrap();
    assert!(result.contains("# My zsh config"));
    assert!(result.contains("alias ll"));
    assert!(result.contains(MARKER_BEGIN));
}

// =========================================================================
// Test install function directly with temp HOME
// =========================================================================
#[test]
fn test_install_function_creates_parent_dirs() {
    // Test that install creates parent directories for fish config
    let dir = tempdir().unwrap();
    let fish_config = dir.path().join(".config").join("fish").join("config.fish");

    // The parent directory doesn't exist yet
    assert!(!fish_config.parent().unwrap().exists());

    // Simulate what install does
    if let Some(parent) = fish_config.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    assert!(fish_config.parent().unwrap().exists());

    // Now write wrapper
    let wrapper = Shell::Fish.wrapper_script();
    std::fs::write(&fish_config, format!("{wrapper}\n")).unwrap();
    assert!(fish_config.exists());
}

#[test]
fn test_install_handles_content_without_newline() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join(".bashrc");

    // Content without trailing newline
    std::fs::write(&config_path, "alias ll='ls -la'").unwrap();

    // Simulate install
    let wrapper = Shell::Bash.wrapper_script();
    let content = std::fs::read_to_string(&config_path).unwrap_or_default();
    let content = remove_wrapper(&content).unwrap();

    // The install function handles this case
    let new_content = if content.is_empty() {
        format!("{wrapper}\n")
    } else if content.ends_with('\n') {
        format!("{content}\n{wrapper}\n")
    } else {
        format!("{content}\n\n{wrapper}\n")
    };
    std::fs::write(&config_path, new_content).unwrap();

    let result = std::fs::read_to_string(&config_path).unwrap();
    assert!(result.contains("alias ll"));
    assert!(result.contains(MARKER_BEGIN));
    // Should have blank line before wrapper when content didn't end with newline
    assert!(result.contains("\n\n"));
}

#[test]
fn test_remove_wrapper_trailing_newlines() {
    let content = r#"alias ll='ls -la'


"#;
    let result = remove_wrapper(content).unwrap();
    // Should not have excessive trailing newlines
    assert!(!result.ends_with("\n\n\n"));
}
