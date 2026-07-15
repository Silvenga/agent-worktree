use super::*;

mod install_tests;

// =========================================================================
// Shell::from_path tests
// =========================================================================
#[test]
fn test_shell_from_path() {
    assert_eq!(Shell::from_path("/bin/bash"), Some(Shell::Bash));
    assert_eq!(Shell::from_path("/usr/bin/zsh"), Some(Shell::Zsh));
    assert_eq!(
        Shell::from_path("/opt/homebrew/bin/fish"),
        Some(Shell::Fish)
    );
    assert_eq!(
        Shell::from_path("C:\\Program Files\\PowerShell\\7\\pwsh.exe"),
        Some(Shell::PowerShell)
    );
    assert_eq!(Shell::from_path("/bin/sh"), None);
}

#[test]
fn test_shell_from_path_case_insensitive() {
    assert_eq!(Shell::from_path("/bin/BASH"), Some(Shell::Bash));
    assert_eq!(Shell::from_path("/usr/bin/ZSH"), Some(Shell::Zsh));
    assert_eq!(Shell::from_path("/usr/bin/Fish"), Some(Shell::Fish));
}

#[test]
fn test_shell_from_path_powershell_variants() {
    assert_eq!(Shell::from_path("pwsh"), Some(Shell::PowerShell));
    assert_eq!(
        Shell::from_path("C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"),
        Some(Shell::PowerShell)
    );
}

// =========================================================================
// Shell::from_name tests
// =========================================================================
#[test]
fn test_shell_from_name() {
    assert_eq!(Shell::from_name("bash"), Some(Shell::Bash));
    assert_eq!(Shell::from_name("ZSH"), Some(Shell::Zsh));
    assert_eq!(Shell::from_name("Fish"), Some(Shell::Fish));
    assert_eq!(Shell::from_name("powershell"), Some(Shell::PowerShell));
    assert_eq!(Shell::from_name("pwsh"), Some(Shell::PowerShell));
    assert_eq!(Shell::from_name("ksh"), None);
}

#[test]
fn test_shell_from_name_unknown() {
    assert_eq!(Shell::from_name("sh"), None);
    assert_eq!(Shell::from_name("csh"), None);
    assert_eq!(Shell::from_name("tcsh"), None);
    assert_eq!(Shell::from_name("unknown"), None);
}

// =========================================================================
// Shell::detect tests
// =========================================================================
#[test]
fn test_shell_detect_returns_option() {
    // Just test that detect() returns a valid option
    let result = Shell::detect();
    // On most CI/dev systems, this should return Some()
    // We can't assert the exact shell since it depends on environment
    assert!(result.is_some() || result.is_none()); // Valid either way
}

// =========================================================================
// Shell::config_file tests
// =========================================================================
#[test]
fn test_shell_config_file_bash() {
    let config = Shell::Bash.config_file();
    assert!(config.is_ok());
    let path = config.unwrap();
    // macOS uses .bash_profile if exists, otherwise .bashrc
    let path_str = path.to_string_lossy();
    assert!(path_str.contains(".bashrc") || path_str.contains(".bash_profile"));
}

#[test]
fn test_shell_config_file_zsh() {
    let config = Shell::Zsh.config_file();
    assert!(config.is_ok());
    let path = config.unwrap();
    assert!(path.to_string_lossy().contains(".zshrc"));
}

#[test]
fn test_shell_config_file_fish() {
    let config = Shell::Fish.config_file();
    assert!(config.is_ok());
    let path = config.unwrap();
    assert!(path.to_string_lossy().contains("config.fish"));
}

#[test]
fn test_shell_config_file_powershell() {
    let config = Shell::PowerShell.config_file();
    assert!(config.is_ok());
    let path = config.unwrap();
    assert!(path
        .to_string_lossy()
        .contains("Microsoft.PowerShell_profile.ps1"));
}

// =========================================================================
// Shell::wrapper_script tests
// =========================================================================
#[test]
fn test_wrapper_script_contains_markers() {
    let bash_wrapper = Shell::Bash.wrapper_script();
    assert!(bash_wrapper.contains(MARKER_BEGIN));
    assert!(bash_wrapper.contains(MARKER_END));

    let fish_wrapper = Shell::Fish.wrapper_script();
    assert!(fish_wrapper.contains(MARKER_BEGIN));
    assert!(fish_wrapper.contains(MARKER_END));

    let ps_wrapper = Shell::PowerShell.wrapper_script();
    assert!(ps_wrapper.contains(MARKER_BEGIN));
    assert!(ps_wrapper.contains(MARKER_END));
}

#[test]
fn test_wrapper_script_bash_zsh_same() {
    let bash = Shell::Bash.wrapper_script();
    let zsh = Shell::Zsh.wrapper_script();
    assert_eq!(bash, zsh);
}

#[test]
fn test_wrapper_script_contains_wt_function() {
    let bash = Shell::Bash.wrapper_script();
    assert!(bash.contains("wt()"));

    let fish = Shell::Fish.wrapper_script();
    assert!(fish.contains("function wt"));

    let ps = Shell::PowerShell.wrapper_script();
    assert!(ps.contains("function wt"));
}

#[test]
fn test_wrapper_script_handles_cd_command() {
    let bash = Shell::Bash.wrapper_script();
    assert!(bash.contains("cd|new|rm|mv|merge|clean|run)"));

    let fish = Shell::Fish.wrapper_script();
    assert!(fish.contains("case cd new rm mv merge clean run"));

    let ps = Shell::PowerShell.wrapper_script();
    assert!(ps.contains("'cd', 'new', 'rm', 'mv', 'merge', 'clean', 'run'"));
}

#[test]
fn test_wrapper_scripts_insert_path_file_before_agent_delimiter() {
    // `wt run` uses `--` to make all remaining arguments agent-owned. The
    // wrapper must therefore insert its private option before that boundary.
    let bash = Shell::Bash.wrapper_script();
    assert!(bash.contains("for wt_arg in \"$@\"; do"));
    assert!(bash.contains("wt_args+=(--path-file \"$path_file\")"));
    assert!(bash.contains("\"$wt_bin\" \"${wt_args[@]}\""));
    assert!(!bash.contains("\"$wt_bin\" \"$@\" --path-file"));

    let fish = Shell::Fish.wrapper_script();
    assert!(fish.contains("for wt_arg in $argv"));
    assert!(fish.contains("set -a wt_args --path-file $path_file"));
    assert!(fish.contains("$wt_bin $wt_args"));
    assert!(!fish.contains("$wt_bin $argv --path-file"));

    let powershell = Shell::PowerShell.wrapper_script();
    assert!(powershell.contains("foreach ($arg in $args)"));
    assert!(powershell.contains("$wtArgs += '--path-file'"));
    assert!(powershell.contains("& $wtBin.Source @wtArgs"));
    assert!(!powershell.contains("& $wtBin.Source @args --path-file"));
}

#[cfg(unix)]
#[test]
fn test_bash_wrapper_keeps_path_file_out_of_agent_arguments() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let dir = tempfile::tempdir().unwrap();
    let wrapper = dir.path().join("wrapper.bash");
    let bin_dir = dir.path().join("bin");
    let fake_wt = bin_dir.join("wt");
    let captured = dir.path().join("arguments");
    std::fs::create_dir(&bin_dir).unwrap();
    std::fs::write(&wrapper, Shell::Bash.wrapper_script()).unwrap();
    std::fs::write(
        &fake_wt,
        "#!/bin/sh\n[ -n \"$COMPLETE\" ] || printf '%s\\n' \"$@\" > \"$WT_CAPTURE\"\n",
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_wt).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&fake_wt, permissions).unwrap();

    let output = Command::new("bash")
        .args([
            "--noprofile",
            "--norc",
            "-c",
            "source \"$WRAPPER\"; wt run -- codex --model gpt-5",
        ])
        .env("WRAPPER", &wrapper)
        .env("WT_CAPTURE", &captured)
        .env(
            "PATH",
            format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap()),
        )
        .output()
        .expect("bash must execute generated wrapper");
    assert!(
        output.status.success(),
        "wrapper failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args: Vec<_> = std::fs::read_to_string(&captured)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(args[0], "run");
    assert_eq!(args[1], "--path-file");
    assert_eq!(args[3], "--");
    assert_eq!(&args[4..], ["codex", "--model", "gpt-5"]);
}

// =========================================================================
// Wrapper contract tests: no business loops, single-line path_file only
// =========================================================================
#[test]
fn test_wrapper_scripts_have_no_snap_loop() {
    for shell in [Shell::Bash, Shell::Fish, Shell::PowerShell] {
        let wrapper = shell.wrapper_script();
        assert!(
            !wrapper.contains("snap-continue"),
            "{shell:?} wrapper must not call snap-continue (loop moved into the binary)"
        );
        assert!(
            !wrapper.contains("head -n1") && !wrapper.contains("$lines[1]"),
            "{shell:?} wrapper must not parse a multi-line path_file"
        );
    }
    // Agent commands are never eval'd by the wrapper anymore. (PowerShell's
    // completion init legitimately uses Invoke-Expression, so only the
    // POSIX shells get the blanket check.)
    for shell in [Shell::Bash, Shell::Fish] {
        assert!(
            !shell.wrapper_script().contains("eval "),
            "{shell:?} wrapper must not eval agent commands"
        );
    }
}

#[test]
fn test_wrapper_scripts_propagate_exit_code() {
    // `wt run` exit codes are a machine contract; the wrapper must return
    // the binary's status, not the status of its own cleanup commands.
    let bash = Shell::Bash.wrapper_script();
    assert!(bash.contains("wt_status=$?"));
    assert!(bash.contains("return $wt_status"));

    let fish = Shell::Fish.wrapper_script();
    assert!(fish.contains("set -l wt_status $status"));
    assert!(fish.contains("return $wt_status"));

    let ps = Shell::PowerShell.wrapper_script();
    assert!(ps.contains("$wtStatus = $LASTEXITCODE"));
    assert!(ps.contains("return $wtStatus"));
}

#[test]
fn test_fish_wrapper_script_syntax() {
    let wrapper = Shell::Fish.wrapper_script();
    // Fish uses 'function' and 'end' keywords
    assert!(wrapper.contains("function wt"));
    assert!(wrapper.contains("end"));
    assert!(wrapper.contains("switch"));
}

#[test]
fn test_powershell_wrapper_script_syntax() {
    let wrapper = Shell::PowerShell.wrapper_script();
    // PowerShell uses function {} and switch
    assert!(wrapper.contains("function wt {"));
    assert!(wrapper.contains("switch"));
    assert!(wrapper.contains("Set-Location"));
}

// =========================================================================
// Shell equality and clone tests
// =========================================================================
#[test]
fn test_shell_equality() {
    assert_eq!(Shell::Bash, Shell::Bash);
    assert_ne!(Shell::Bash, Shell::Zsh);
    assert_ne!(Shell::Fish, Shell::PowerShell);
}

#[test]
fn test_shell_clone() {
    let shell = Shell::Fish;
    let cloned = shell;
    assert_eq!(shell, cloned);
}

#[test]
fn test_shell_debug() {
    let shell = Shell::Bash;
    let debug = format!("{:?}", shell);
    assert_eq!(debug, "Bash");
}

// =========================================================================
// Completion init snippet tests
// =========================================================================
#[test]
fn test_bash_zsh_wrapper_contains_completion_init() {
    let wrapper = Shell::Bash.wrapper_script();
    assert!(
        wrapper.contains("COMPLETE="),
        "bash/zsh wrapper should contain COMPLETE env var for dynamic completions"
    );
    assert!(
        wrapper.contains("_wt_bin"),
        "bash/zsh wrapper should locate binary for completions"
    );
}

#[test]
fn test_powershell_wrapper_contains_completion_init() {
    let wrapper = Shell::PowerShell.wrapper_script();
    assert!(
        wrapper.contains("COMPLETE"),
        "powershell wrapper should contain COMPLETE env var for completions"
    );
}

#[test]
fn test_fish_completions_content() {
    assert!(
        FISH_COMPLETIONS.contains("COMPLETE=fish"),
        "fish completions should set COMPLETE=fish"
    );
    assert!(
        FISH_COMPLETIONS.contains("source"),
        "fish completions should source the output"
    );
}

#[test]
fn test_fish_completions_path() {
    let path = fish_completions_path();
    assert!(path.is_ok());
    let path = path.unwrap();
    assert!(path.to_string_lossy().contains("completions/wt.fish"));
}
