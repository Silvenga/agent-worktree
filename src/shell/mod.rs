// ===========================================================================
// shell - Shell Integration Installation
// ===========================================================================

use std::path::PathBuf;

use directories::BaseDirs;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("home directory not found")]
    NoHome,

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    PowerShell,
}

impl Shell {
    pub fn detect() -> Option<Self> {
        // Windows: default to PowerShell
        #[cfg(windows)]
        {
            return Some(Shell::PowerShell);
        }

        // Unix: check $SHELL
        #[cfg(not(windows))]
        {
            std::env::var("SHELL")
                .ok()
                .and_then(|s| Self::from_path(&s))
        }
    }

    pub fn from_path(path: &str) -> Option<Self> {
        let path_lower = path.to_lowercase();
        if path_lower.ends_with("bash") {
            Some(Shell::Bash)
        } else if path_lower.ends_with("zsh") {
            Some(Shell::Zsh)
        } else if path_lower.ends_with("fish") {
            Some(Shell::Fish)
        } else if path_lower.contains("powershell") || path_lower.ends_with("pwsh") {
            Some(Shell::PowerShell)
        } else {
            None
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "bash" => Some(Shell::Bash),
            "zsh" => Some(Shell::Zsh),
            "fish" => Some(Shell::Fish),
            "powershell" | "pwsh" => Some(Shell::PowerShell),
            _ => None,
        }
    }

    pub fn config_file(&self) -> Result<PathBuf> {
        let base = BaseDirs::new().ok_or(Error::NoHome)?;
        let home = base.home_dir();

        Ok(match self {
            Shell::Bash => {
                // macOS: login shells read .bash_profile, not .bashrc
                // Use .bash_profile if it exists, otherwise .bashrc
                #[cfg(target_os = "macos")]
                {
                    let bash_profile = home.join(".bash_profile");
                    if bash_profile.exists() {
                        bash_profile
                    } else {
                        home.join(".bashrc")
                    }
                }
                #[cfg(not(target_os = "macos"))]
                {
                    home.join(".bashrc")
                }
            }
            Shell::Zsh => home.join(".zshrc"),
            Shell::Fish => home.join(".config/fish/config.fish"),
            // PowerShell profile: $HOME/Documents/PowerShell/Microsoft.PowerShell_profile.ps1
            Shell::PowerShell => {
                #[cfg(windows)]
                {
                    home.join("Documents")
                        .join("PowerShell")
                        .join("Microsoft.PowerShell_profile.ps1")
                }
                #[cfg(not(windows))]
                {
                    // On Unix, pwsh uses ~/.config/powershell/Microsoft.PowerShell_profile.ps1
                    home.join(".config")
                        .join("powershell")
                        .join("Microsoft.PowerShell_profile.ps1")
                }
            }
        })
    }

    pub fn wrapper_script(&self) -> &'static str {
        match self {
            Shell::Bash | Shell::Zsh => BASH_ZSH_WRAPPER,
            Shell::Fish => FISH_WRAPPER,
            Shell::PowerShell => POWERSHELL_WRAPPER,
        }
    }
}

const MARKER_BEGIN: &str = "# === agent-worktree BEGIN ===";
const MARKER_END: &str = "# === agent-worktree END ===";

// ---------------------------------------------------------------------------
// Shell wrapper scripts
//
// Wrapper contract (keep all three scripts in sync):
// - The wrapper's only job: run the binary, then cd if the binary wrote a
//   non-empty path_file (single line = target path). No business loops here —
//   interactive sessions run inside the binary (`wt run -i`).
// - Exit codes are propagated verbatim: `wt run` codes are a machine
//   contract (see cli/commands/run/headless.rs).
// ---------------------------------------------------------------------------

const BASH_ZSH_WRAPPER: &str = r#"# === agent-worktree BEGIN ===
# NOTE: Don't use 'path'/'status' as variable names - zsh reserves them
wt() {
  local wt_bin path_file target_path wt_status
  if [[ -n "$ZSH_VERSION" ]]; then
    wt_bin=$(whence -p wt 2>/dev/null)
  else
    wt_bin=$(type -P wt 2>/dev/null)
  fi
  if [[ -z "$wt_bin" ]]; then
    echo "wt: binary not found. Install: npm install -g agent-worktree" >&2
    return 1
  fi
  # Pass through if -h/--help anywhere in args
  case " $* " in
    *" -h "*|*" --help "*) "$wt_bin" "$@"; return ;;
  esac
  case "$1" in
    cd|new|rm|mv|merge|clean|run)
      # Use mktemp so concurrent calls (and subshells where $$ is the parent
      # PID) get unique files; fall back to PID-based name if mktemp missing.
      path_file=$(mktemp 2>/dev/null) || path_file="${TMPDIR:-/tmp}/wt-path-$$"
      "$wt_bin" "$@" --path-file "$path_file"
      wt_status=$?
      # -s guards the empty file mktemp created: cd only on a written target
      if [[ $wt_status -eq 0 && -s "$path_file" ]]; then
        target_path=$(<"$path_file"); cd "$target_path"
      fi
      rm -f "$path_file"
      return $wt_status
      ;;
    *)
      "$wt_bin" "$@"
      ;;
  esac
}
# Dynamic completions: call binary directly to bypass wt function
if [[ -n "$ZSH_VERSION" ]]; then
  _wt_bin=$(whence -p wt 2>/dev/null)
  [[ -n "$_wt_bin" ]] && source <(COMPLETE=zsh "$_wt_bin" 2>/dev/null) 2>/dev/null
else
  _wt_bin=$(type -P wt 2>/dev/null)
  [[ -n "$_wt_bin" ]] && source <(COMPLETE=bash "$_wt_bin" 2>/dev/null) 2>/dev/null
fi
unset _wt_bin
# === agent-worktree END ==="#;

const FISH_WRAPPER: &str = r#"# === agent-worktree BEGIN ===
function wt
  set -l wt_bin (type --force-path wt 2>/dev/null)
  if test -z "$wt_bin"
    echo "wt: binary not found. Install: npm install -g agent-worktree" >&2
    return 1
  end
  if contains -- -h $argv; or contains -- --help $argv
    $wt_bin $argv
    return
  end
  switch $argv[1]
    case cd new rm mv merge clean run
      set -l path_file (mktemp)
      $wt_bin $argv --path-file $path_file
      set -l wt_status $status
      # -s guards the empty file mktemp created: cd only on a written target
      if test $wt_status -eq 0; and test -s $path_file
        cd (cat $path_file)
      end
      rm -f $path_file
      return $wt_status
    case '*'
      $wt_bin $argv
  end
end
# === agent-worktree END ==="#;

const POWERSHELL_WRAPPER: &str = r#"# === agent-worktree BEGIN ===
function wt {
  $wtBin = Get-Command wt -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $wtBin) {
    Write-Error "wt: binary not found. Install: npm install -g agent-worktree"
    return 1
  }
  if ($args -contains '-h' -or $args -contains '--help') {
    & $wtBin.Source @args
    return
  }
  switch ($args[0]) {
    { $_ -in 'cd', 'new', 'rm', 'mv', 'merge', 'clean', 'run' } {
      $pathFile = [System.IO.Path]::GetTempFileName()
      & $wtBin.Source @args --path-file $pathFile
      $wtStatus = $LASTEXITCODE
      # cd only on a written (non-empty) target - GetTempFileName pre-creates the file
      if ($wtStatus -eq 0 -and (Test-Path $pathFile)) {
        $target = Get-Content $pathFile -Raw -ErrorAction SilentlyContinue
        if ($target) { Set-Location $target.Trim() }
      }
      Remove-Item $pathFile -ErrorAction SilentlyContinue
      if ($wtStatus -ne 0) { return $wtStatus }
    }
    default {
      & $wtBin.Source @args
    }
  }
}
# Dynamic completions: call binary directly to bypass wt function
$_wtBin = Get-Command wt -CommandType Application -ErrorAction SilentlyContinue |
  Select-Object -ExpandProperty Source -First 1
if ($_wtBin) {
  $env:COMPLETE = "powershell"
  & $_wtBin | Out-String | Invoke-Expression
  Remove-Item Env:\COMPLETE -ErrorAction SilentlyContinue
}
Remove-Variable _wtBin -ErrorAction SilentlyContinue
# === agent-worktree END ==="#;

// Fish completions go to a dedicated file (auto-sourced by fish)
const FISH_COMPLETIONS: &str = r#"# Dynamic completions for wt (auto-generated by wt setup)
set -l _wt_bin (type --force-path wt 2>/dev/null)
if test -n "$_wt_bin"
  COMPLETE=fish $_wt_bin | source
end
"#;

/// Fish completions file path: ~/.config/fish/completions/wt.fish
fn fish_completions_path() -> Result<PathBuf> {
    let base = BaseDirs::new().ok_or(Error::NoHome)?;
    Ok(base.home_dir().join(".config/fish/completions/wt.fish"))
}

/// Install shell wrapper to config file
pub fn install(shell: Shell) -> Result<()> {
    let config_path = shell.config_file()?;
    let wrapper = shell.wrapper_script();

    // Ensure parent directory exists (for fish)
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Read existing content or empty
    let content = std::fs::read_to_string(&config_path).unwrap_or_default();

    // Remove old wrapper if present
    let content = remove_wrapper(&content)?;

    // Append new wrapper with blank lines before and after
    let new_content = if content.is_empty() {
        format!("{wrapper}\n")
    } else if content.ends_with('\n') {
        format!("{content}\n{wrapper}\n")
    } else {
        format!("{content}\n\n{wrapper}\n")
    };

    std::fs::write(&config_path, new_content)?;

    // Fish: also install dedicated completions file
    if shell == Shell::Fish {
        let completions_path = fish_completions_path()?;
        if let Some(parent) = completions_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&completions_path, FISH_COMPLETIONS)?;
    }

    Ok(())
}

/// Strip an existing wrapper block from rc-file content.
///
/// Refuses to touch the file if BEGIN/END markers don't pair up cleanly —
/// silently truncating after an orphan marker would wipe unrelated user
/// config (PATH exports, aliases, etc).
fn remove_wrapper(content: &str) -> Result<String> {
    let mut result = String::new();
    let mut in_wrapper = false;

    for (idx, line) in content.lines().enumerate() {
        if line.contains(MARKER_BEGIN) {
            if in_wrapper {
                return Err(Error::Other(format!(
                    "rc file has nested '{MARKER_BEGIN}' at line {} — manual fixup needed before re-running 'wt setup'.",
                    idx + 1
                )));
            }
            in_wrapper = true;
            continue;
        }
        if line.contains(MARKER_END) {
            if !in_wrapper {
                return Err(Error::Other(format!(
                    "rc file has '{MARKER_END}' without matching BEGIN at line {} — manual fixup needed before re-running 'wt setup'.",
                    idx + 1
                )));
            }
            in_wrapper = false;
            continue;
        }
        if !in_wrapper {
            result.push_str(line);
            result.push('\n');
        }
    }

    if in_wrapper {
        return Err(Error::Other(format!(
            "rc file has '{MARKER_BEGIN}' without matching END — manual fixup needed before re-running 'wt setup'."
        )));
    }

    // Remove trailing newlines
    while result.ends_with("\n\n") {
        result.pop();
    }

    Ok(result)
}

#[cfg(test)]
mod tests;
