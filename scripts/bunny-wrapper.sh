#!/usr/bin/env bash
# Run the bunny CLI from a git checkout (release binary, debug binary, or cargo build).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RELEASE="$ROOT/target/release/bunny"
DEBUG="$ROOT/target/debug/bunny"

source_cargo_env() {
  # rustup installs to ~/.cargo/bin; subprocess installs do not update this shell's PATH.
  # Use if/then: with set -e, a false [[ ]] as the last statement in a function exits the script.
  if [[ -f "${HOME}/.cargo/env" ]]; then
    # shellcheck source=/dev/null
    source "${HOME}/.cargo/env"
  fi
}

source_cargo_env

runnable() {
  local bin="$1"
  # Require the `run` subcommand (ignore stale release binaries from older trees).
  [[ -x "$bin" ]] && "$bin" run --help >/dev/null 2>&1
}

server_sources_newer_than_release() {
  [[ -f "$RELEASE" ]] || return 1
  find "$ROOT/apps/server/src" "$ROOT/crates" \
    \( -name '*.rs' -o -name 'Cargo.toml' \) \
    -newer "$RELEASE" -print -quit 2>/dev/null | grep -q .
}

build_release_binary() {
  if runnable "$RELEASE" && [[ "${BUNNY_FORCE_BUILD:-}" != "1" ]]; then
    if ! server_sources_newer_than_release; then
      return 0
    fi
    echo "→ Server sources changed since last build — recompiling…" >&2
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
  shift
  SETUP_SKIP_PREREQS=0
  SETUP_MINIMAL=0
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --minimal)
        SETUP_MINIMAL=1
        shift
        ;;
      --skip-prerequisites | --skip-prereqs)
        SETUP_SKIP_PREREQS=1
        shift
        ;;
      *)
        echo "bunny setup: unknown option: $1" >&2
        echo "  Usage: ./bunny setup [--minimal] [--skip-prerequisites]" >&2
        exit 1
        ;;
    esac
  done

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

  if [[ "$SETUP_SKIP_PREREQS" -eq 0 ]] && ! command -v cargo >/dev/null 2>&1 && command -v apt-get >/dev/null 2>&1; then
    prereq_args=()
    [[ "$SETUP_MINIMAL" -eq 1 ]] && prereq_args+=(--minimal)
    echo "→ Rust not found — installing prerequisites (Debian/Ubuntu)…" >&2
    "$ROOT/scripts/install-prerequisites.sh" "${prereq_args[@]}"
    source_cargo_env
  fi

  if command -v cargo >/dev/null 2>&1; then
    build_release_binary
  else
    echo "⚠ Rust not installed — run: ./scripts/install-prerequisites.sh (or ./bunny setup on Debian/Ubuntu)"
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

cli_supports_discord() {
  local bin="$1"
  [[ -x "$bin" ]] && "$bin" --help 2>&1 | grep -q 'discord'
}

bunny_docker_dev() {
  [[ "${BUNNY_DOCKER_DEV:-}" == 1 ]] || [[ -f /.dockerenv ]]
}

# Fresh Docker dev containers start without Rust; install on first `bunny` use.
ensure_docker_toolchain() {
  if ! bunny_docker_dev; then
    return 0
  fi
  source_cargo_env
  if command -v cargo >/dev/null 2>&1; then
    return 0
  fi
  if runnable "$RELEASE" && cli_supports_discord "$RELEASE"; then
    return 0
  fi
  if [[ ! -f "$ROOT/scripts/install-prerequisites.sh" ]]; then
    return 0
  fi
  if ! command -v apt-get >/dev/null 2>&1; then
    return 0
  fi
  echo "→ Docker: installing Rust and build tools (first time, several minutes)…" >&2
  "$ROOT/scripts/install-prerequisites.sh" --minimal
  source_cargo_env
}

ensure_docker_toolchain

# Use an existing build without Rust (e.g. after `./bunny setup` in another shell, or Docker init).
if runnable "$RELEASE"; then
  # Docker dev: sources on the mount may be newer than target/release — still run the binary if we cannot rebuild.
  if bunny_docker_dev && ! command -v cargo >/dev/null 2>&1; then
    exec_bunny "$RELEASE" "$@"
  fi
  if ! server_sources_newer_than_release && cli_supports_discord "$RELEASE"; then
    exec_bunny "$RELEASE" "$@"
  fi
  if ! cli_supports_discord "$RELEASE"; then
    echo "→ Rebuilding bunny (CLI missing discord commands)…" >&2
  fi
fi

if runnable "$DEBUG" && cli_supports_discord "$DEBUG"; then
  exec_bunny "$DEBUG" "$@"
fi

if ! command -v cargo >/dev/null 2>&1; then
  ensure_docker_toolchain
fi
if ! command -v cargo >/dev/null 2>&1; then
  echo "bunny: Rust toolchain required — run: ./bunny setup (or ./scripts/install-prerequisites.sh)" >&2
  exit 1
fi

if ! runnable "$RELEASE" || server_sources_newer_than_release || ! cli_supports_discord "$RELEASE"; then
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
