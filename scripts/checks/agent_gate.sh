#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# agent_gate.sh — deterministic enforcement of CLAUDE.local.md rules,
# compiled by prompt-to-code. Exits 2 on any violation so a Claude Code hook
# blocks the turn and re-prompts the agent with the rule message; exit 0 = clean.
#
# Rules enforced (each names itself on failure):
#   - "Lint + Format zero warnings"  -> cargo fmt --check + cargo clippy -D warnings
#   - "files stay <= 500 lines"      -> tests/repo_hygiene.rs
#   - "comments/code text English"   -> tests/repo_hygiene.rs
#
# Usage: agent_gate.sh [--fast]
#   --fast : only the text-level checks (length + English-only) via the
#            repo_hygiene test; cheap enough to run after every edit.
#   (none) : full gate — adds fmt + clippy. The Stop / pre-commit wall.
# ---------------------------------------------------------------------------
set -uo pipefail

root=$(git rev-parse --show-toplevel 2>/dev/null) || {
  echo "agent_gate: not inside a git repository" >&2
  exit 2
}
cd "$root" || exit 2

fail() {
  echo "BLOCKED by project rule — $1" >&2
  exit 2
}

# Rules "files stay <= 500 lines" and "comments/code text English"
# (native, self-testing check; also the drift detector).
cargo test --test repo_hygiene --quiet >&2 \
  || fail "'files stay <= 500 lines' or 'comments/code text English' — see the failing assertion above."

if [ "${1:-}" != "--fast" ]; then
  # Rule "Lint + Format zero warnings".
  cargo fmt --check >&2 \
    || fail "'Lint + Format zero warnings' — run 'cargo fmt' to fix formatting."
  cargo clippy --all-targets -- -D warnings >&2 \
    || fail "'Lint + Format zero warnings' — clippy reported warnings; fix them."
fi

exit 0
