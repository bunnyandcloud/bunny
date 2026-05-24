#!/usr/bin/env bash
# Optional helper — the repo works without this script (use ./bunny from the clone root).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MARKER="# bunny dev PATH"
PATH_LINE="export PATH=\"$ROOT:\${PATH}\""

chmod +x "$ROOT/bunny" "$ROOT/scripts/bunny-wrapper.sh" 2>/dev/null || true

activate_path() {
  export PATH="$ROOT:${PATH:-}"
}

install_to_shell_rc() {
  local rc="$1"
  touch "$rc"
  if grep -qF "$MARKER" "$rc" 2>/dev/null; then
    echo "✓ PATH already in $rc"
    return
  fi
  {
    echo ""
    echo "$MARKER"
    echo "$PATH_LINE"
  } >>"$rc"
  echo "✓ Added bunny to PATH in $rc (open a new terminal to pick it up)"
}

# Sourced: only affect the current shell (no .bashrc edit).
if [[ "${BASH_SOURCE[0]}" != "${0}" ]]; then
  activate_path
  echo "✓ bunny is on PATH in this shell — try: bunny --help"
  return 0 2>/dev/null || exit 0
fi

# Executed: show the simple default workflow.
case "${1:-}" in
  --install)
    pick_rc() {
      if [[ -n "${SHELL:-}" ]]; then
        case "$SHELL" in
          */zsh) echo "${ZDOTDIR:-$HOME}/.zshrc" ;;
          */bash) echo "$HOME/.bashrc" ;;
          *) echo "$HOME/.profile" ;;
        esac
      elif [[ -f "$HOME/.zshrc" ]]; then
        echo "$HOME/.zshrc"
      else
        echo "$HOME/.bashrc"
      fi
    }
    install_to_shell_rc "$(pick_rc)"
    ;;
  --help|-h)
    cat <<EOF
Usage:
  ./bunny configure              # no setup script needed
  source ./scripts/setup-dev.sh  # optional: type "bunny" without ./ in this shell
  ./scripts/setup-dev.sh --install   # optional: add repo to PATH in ~/.bashrc / ~/.zshrc
EOF
    ;;
  *)
    cat <<EOF
No setup required. From the repository root:

  ./bunny configure
  ./bunny run

Optional — use "bunny" without ./ in the current shell:

  source ./scripts/setup-dev.sh

Optional — persist PATH in your shell profile:

  ./scripts/setup-dev.sh --install
EOF
    ;;
esac
