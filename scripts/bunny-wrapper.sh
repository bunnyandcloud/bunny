#!/usr/bin/env bash
# Run the bunny CLI from a git checkout (release binary, debug binary, or cargo build).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RELEASE="$ROOT/target/release/bunny"
DEBUG="$ROOT/target/debug/bunny"

runnable() {
  local bin="$1"
  # Require the `run` subcommand (ignore stale release binaries from older trees).
  [[ -x "$bin" ]] && "$bin" run --help >/dev/null 2>&1
}

build_release_binary() {
  if runnable "$RELEASE"; then
    return 0
  fi
  if [[ "${BUNNY_VERBOSE_BUILD:-}" == "1" ]]; then
    echo "→ Compiling bunny agent…" >&2
    (cd "$ROOT" && cargo build --release -p bunny-server)
  else
    echo "→ Compiling bunny agent (one-time, a few minutes)…" >&2
    (cd "$ROOT" && RUSTFLAGS="${RUSTFLAGS:-} -Awarnings" CARGO_TERM_QUIET=true cargo build --release -p bunny-server -q)
  fi
  echo "✓ Bunny agent built" >&2
}

# Docker publishes ports to the container network — 127.0.0.1 inside the container is not reachable from the host.
exec_bunny() {
  local bin="$1"
  shift
  if [[ -f /.dockerenv ]] && [[ $# -gt 0 ]]; then
    local case_cmd="$1"
    case "$case_cmd" in
      run | start | dev)
        local has_host=0 arg
        for arg in "$@"; do
          [[ "$arg" == "--host" ]] && has_host=1
        done
        if [[ $has_host -eq 0 ]]; then
          echo "✓ Docker — binding 0.0.0.0 (open http://127.0.0.1:<port> on your machine)" >&2
          set -- "$1" --host 0.0.0.0 "${@:2}"
        fi
        ;;
    esac
  fi
  exec "$bin" "$@"
}

# Handled by the shell launcher (not the Rust binary).
if [[ "${1:-}" == "setup" ]]; then
  PATH_MARKER="# bunny CLI PATH"

  pick_install_dir() {
    if [[ -n "${INSTALL_DIR:-}" ]]; then
      echo "$INSTALL_DIR"
      return
    fi
    if [[ -d /usr/local/bin ]] && [[ -w /usr/local/bin ]]; then
      echo /usr/local/bin
      return
    fi
    echo "$HOME/.local/bin"
  }

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

  INSTALL_DIR="$(pick_install_dir)"
  mkdir -p "$INSTALL_DIR"
  ln -sf "$ROOT/bunny" "$INSTALL_DIR/bunny"
  echo "✓ Linked bunny → $INSTALL_DIR/bunny"
  echo "  Uses this git checkout: $ROOT"

  if [[ "$INSTALL_DIR" == /usr/local/bin ]]; then
    echo "  Install type: system PATH (/usr/local/bin, writable by you)"
  else
    echo "  Install type: user PATH (~/.local/bin)"
    echo "  Tip: for immediate system-wide CLI without editing your profile, run: sudo ./bunny setup"
  fi

  if [[ "$INSTALL_DIR" == "$HOME/.local/bin" ]]; then
    RC="$(pick_rc)"
    PATH_LINE="export PATH=\"$INSTALL_DIR:\${PATH}\""
    touch "$RC"
    if grep -qF "$PATH_MARKER" "$RC" 2>/dev/null; then
      echo "✓ PATH already configured in $RC"
    else
      {
        echo ""
        echo "$PATH_MARKER"
        echo "$PATH_LINE"
      } >>"$RC"
      echo "✓ Added $INSTALL_DIR to PATH in $RC (new shells)"
    fi
  fi

  if command -v cargo >/dev/null 2>&1; then
    # shellcheck source=/dev/null
    [[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env"
    build_release_binary
  else
    echo "⚠ Rust not installed — run: ./scripts/install-prerequisites.sh"
  fi

  echo ""
  if [[ ":${PATH}:" == *":$INSTALL_DIR:"* ]]; then
    echo "✓ Ready — run: bunny configure"
  else
    echo "⚠ PATH not updated in this shell."
    echo "  Open a new terminal, or run:  source \"$(pick_rc)\""
  fi
  exit 0
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "bunny: Rust toolchain required — run: ./scripts/install-prerequisites.sh" >&2
  exit 1
fi
# shellcheck source=/dev/null
[[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env"

if ! runnable "$RELEASE"; then
  build_release_binary
fi

if runnable "$RELEASE"; then
  exec_bunny "$RELEASE" "$@"
fi

if runnable "$DEBUG"; then
  exec_bunny "$DEBUG" "$@"
fi

echo "bunny: build failed — try: cd $ROOT && cargo build --release -p bunny-server" >&2
exit 1
