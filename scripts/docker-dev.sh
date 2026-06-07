#!/usr/bin/env bash
# Bunny Docker dev — one entry point.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
COMPOSE_FILE="${ROOT}/docker-compose.dev.yml"
ENV_FILE="${ROOT}/.discord/.env"

ensure_discord_env() {
  mkdir -p "$ROOT/.discord"
  if [[ ! -f "$ENV_FILE" ]]; then
    if [[ -f "$ROOT/.discord/env.example" ]]; then
      cp "$ROOT/.discord/env.example" "$ENV_FILE"
    else
      : >"$ENV_FILE"
    fi
  fi

  # shellcheck source=/dev/null
  source "$ENV_FILE" 2>/dev/null || true

  if [[ -z "${DISCORD_APPLICATION_ID:-}" ]]; then
    read -r -p "Discord Application ID: " DISCORD_APPLICATION_ID
  fi
  if [[ -z "${DISCORD_BOT_TOKEN:-}" ]]; then
    read -r -s -p "Discord Bot Token: " DISCORD_BOT_TOKEN
    echo ""
  fi

  if [[ -z "${DISCORD_APPLICATION_ID:-}" || -z "${DISCORD_BOT_TOKEN:-}" ]]; then
    echo "Missing Discord credentials."
    exit 1
  fi

  # Write back to disk (keep it out of git via .gitignore).
  {
    echo "DISCORD_APPLICATION_ID=${DISCORD_APPLICATION_ID}"
    echo "DISCORD_BOT_TOKEN=${DISCORD_BOT_TOKEN}"
  } >"$ENV_FILE"
  chmod 600 "$ENV_FILE" 2>/dev/null || true
}

# Discord bridge needs outbound DNS (discord.com, gateway.discord.gg).
check_container_dns() {
  if ! docker compose -f "$COMPOSE_FILE" exec -T bunny-dev bash -lc \
    'getent hosts discord.com >/dev/null && getent hosts gateway.discord.gg >/dev/null'; then
    echo "✗ Le conteneur ne peut pas résoudre discord.com (DNS)."
    echo "  Diagnostic: ./scripts/docker-dev.sh check-network"
    echo "  Puis recrée le conteneur: ./scripts/docker-dev.sh down && ./scripts/docker-dev.sh up"
    exit 1
  fi
}

cmd="${1:-help}"
shift || true

case "$cmd" in
  up)
    docker compose -f "$COMPOSE_FILE" up -d
    echo ""
    echo "Next (recommended): ./scripts/docker-dev.sh bootstrap"
    echo "  or: ./scripts/docker-dev.sh shell → bunny configure"
    ;;
  init)
    docker compose -f "$COMPOSE_FILE" up -d
    echo "→ Ensuring config + owner in container…"
    echo "→ First run may install Rust + build the agent (several minutes)…"
    docker compose -f "$COMPOSE_FILE" exec bunny-dev bash -lc '
      set -e
      cd /opt/bunny
      BUNNY=bunny
      [[ -f ~/.cargo/env ]] && source ~/.cargo/env

      bunny_ready() {
        [[ -x target/release/bunny ]] && target/release/bunny run --help >/dev/null 2>&1
      }

      bunny_cli_current() {
        bunny_ready && $BUNNY --help 2>&1 | grep -q "config-init"
      }

      if ! bunny_cli_current; then
        if ! command -v cargo >/dev/null 2>&1; then
          echo "→ Installing prerequisites (Rust, etc.)…"
          $BUNNY setup --minimal
          [[ -f ~/.cargo/env ]] && source ~/.cargo/env
        fi
        if ! bunny_cli_current; then
          echo "→ Building Linux agent (first run or CLI out of date)…"
          cargo build --release -p bunny-server -q
        fi
      fi

      $BUNNY config-init
      $BUNNY configure || true
    '
    echo ""
    echo "→ Start agent (in container):  ./scripts/docker-dev.sh shell"
    echo "   then:  bunny run"
    echo ""
    echo "→ Discord (on Mac, from repo root):"
    echo "   1. Copy .discord/env.example → .discord/.env and fill DISCORD_*"
    echo "   2. ./scripts/docker-dev.sh discord-setup"
    echo "   3. ./scripts/run-discord-bridge.sh"
    ;;
  discord-setup)
    ensure_discord_env
    set -a
    export DISCORD_APPLICATION_ID DISCORD_BOT_TOKEN
    set +a
    docker compose -f "$COMPOSE_FILE" exec \
      -e DISCORD_APPLICATION_ID -e DISCORD_BOT_TOKEN \
      bunny-dev bash -lc '
        cd /opt/bunny
        bunny discord setup --bridge-out /opt/bunny/.discord/bridge.yaml
      '
    echo ""
    echo "✓ Run on Mac: ./scripts/run-discord-bridge.sh"
    ;;
  discord-dev)
    # One-command local dev: run BOTH agent + discord bridge in the container.
    # Only host step: we will prompt and write .discord/.env if missing.
    ensure_discord_env
    set -a
    export DISCORD_APPLICATION_ID DISCORD_BOT_TOKEN
    set +a

    docker compose -f "$COMPOSE_FILE" up -d
    "$0" init

    echo ""
    echo "→ Writing Discord config (in container)…"
    docker compose -f "$COMPOSE_FILE" exec \
      -e DISCORD_APPLICATION_ID -e DISCORD_BOT_TOKEN \
      bunny-dev bash -lc '
        set -e
        cd /opt/bunny
        bunny discord setup --bridge-out /opt/bunny/.discord/bridge.yaml
      '

    echo ""
    echo "→ Starting agent + bridge (in container)…"
    docker compose -f "$COMPOSE_FILE" exec bunny-dev bash -lc '
      cd /opt/bunny
      [[ -f ~/.cargo/env ]] && source ~/.cargo/env
      # nohup + setsid: survive after docker exec exits (plain & gets SIGHUP)
      if ! curl -sf "http://127.0.0.1:7681/api/v1/agent/info" >/dev/null 2>&1; then
        nohup setsid bunny run </dev/null >>/tmp/bunny-agent.log 2>&1 &
        sleep 2
      fi
      export BUNNY_DISCORD_BRIDGE_CONFIG="/opt/bunny/.discord/bridge.yaml"
      export RUST_LOG="${RUST_LOG:-bunny_discord_bridge=info,serenity=warn}"
      pkill -x bunny-discord-bridge 2>/dev/null || true
      sleep 1
      if ! pgrep -x bunny-discord-bridge >/dev/null 2>&1; then
        nohup setsid bunny discord bridge </dev/null >>/tmp/bunny-discord-bridge.log 2>&1 &
        sleep 2
      fi
    '

    echo ""
    if curl -sf "http://127.0.0.1:7681/api/v1/agent/info" >/dev/null 2>&1; then
      echo "✓ Bunny:  http://127.0.0.1:7681"
    else
      echo "⚠ Agent not responding yet — in container run:  bunny run"
      echo "  (or: ./scripts/docker-dev.sh agent-logs)"
    fi
    echo "✓ Discord bridge started (see bridge-logs if /bunny does not reply)"
    echo ""
    echo "Tip: logs"
    echo "  ./scripts/docker-dev.sh bridge-logs"
    echo "  ./scripts/docker-dev.sh agent-logs"
    ;;
  bridge-logs)
    docker compose -f "$COMPOSE_FILE" exec bunny-dev bash -lc '
      if [[ -f /tmp/bunny-discord-bridge.log ]]; then
        tail -n 200 /tmp/bunny-discord-bridge.log
      elif pgrep -x bunny-discord-bridge >/dev/null 2>&1; then
        echo "✓ Bridge running (foreground — logs are in the start-bridge terminal, not a file):"
        pgrep -x bunny-discord-bridge -a
      else
        echo "⚠ No bridge log file and no running bridge process."
        echo "  Start: ./scripts/docker-dev.sh start-bridge"
      fi
    '
    ;;
  agent-logs)
    docker compose -f "$COMPOSE_FILE" exec bunny-dev bash -lc 'tail -n 200 /tmp/bunny-agent.log 2>/dev/null || true'
    ;;
  start-agent)
    docker compose -f "$COMPOSE_FILE" exec bunny-dev bash -lc '
      cd /opt/bunny
      [[ -f ~/.cargo/env ]] && source ~/.cargo/env
      if curl -sf "http://127.0.0.1:7681/api/v1/agent/info" >/dev/null 2>&1; then
        echo "✓ Agent already running"
        exit 0
      fi
      echo "→ Starting agent (foreground — Ctrl+C to stop)…"
      exec bunny run
    '
    ;;
  check-network)
    docker compose -f "$COMPOSE_FILE" exec bunny-dev bash -lc '
      echo "=== /etc/resolv.conf ==="
      cat /etc/resolv.conf
      echo ""
      echo "=== discord.com ==="
      getent hosts discord.com || echo "(échec résolution)"
      echo "=== gateway.discord.gg ==="
      getent hosts gateway.discord.gg || echo "(échec résolution)"
      if command -v curl >/dev/null 2>&1; then
        echo ""
        echo "=== API gateway (HTTPS) ==="
        curl -sfI --max-time 10 https://discord.com/api/v10/gateway | head -3 || echo "(échec HTTP)"
      fi
    '
    ;;
  stop-bridge)
    docker compose -f "$COMPOSE_FILE" exec -T bunny-dev bash -lc '
      pkill -x bunny-discord-bridge 2>/dev/null || true
      sleep 1
      if pgrep -x bunny-discord-bridge >/dev/null 2>&1; then
        echo "⚠ Bridge still running:"
        pgrep -x bunny-discord-bridge -a
        echo "  → Ctrl+C in the terminal where start-bridge is running."
      else
        echo "✓ Bridge stopped (start again: ./scripts/docker-dev.sh start-bridge)"
      fi
    '
    ;;
  start-bridge)
    check_container_dns
    docker compose -f "$COMPOSE_FILE" exec bunny-dev bash -lc '
      cd /opt/bunny
      [[ -f ~/.cargo/env ]] && source ~/.cargo/env
      pkill -x bunny-discord-bridge 2>/dev/null || true
      sleep 1
      cargo build -p bunny-discord-bridge -p bunny-server -q
      export BUNNY_DISCORD_BRIDGE_CONFIG="/opt/bunny/.discord/bridge.yaml"
      exec bunny discord bridge
    '
    ;;
  down)
    docker compose -f "$COMPOSE_FILE" down "$@"
    ;;
  reset)
    echo "→ Stopping container and removing Docker volume (bunny-config)…"
    docker compose -f "$COMPOSE_FILE" down -v
    echo "→ Removing Discord dev files…"
    rm -f "${ROOT}/.discord/.env" "${ROOT}/.discord/bridge.yaml"
    echo "→ Removing host config (~/.config/bunny)…"
    rm -rf "${HOME}/.config/bunny"
    echo "→ Removing build artifacts (target/, apps/web/dist, apps/web/node_modules)…"
    rm -rf "${ROOT}/target" "${ROOT}/apps/web/dist" "${ROOT}/apps/web/node_modules"
    echo ""
    echo "✓ Reset complete."
    ;;
  shell|bash)
    docker compose -f "$COMPOSE_FILE" exec -it bunny-dev bash -lc '
      cd /opt/bunny
      export BUNNY_DOCKER_DEV=1
      [[ -f ~/.cargo/env ]] && source ~/.cargo/env
      if ! command -v cargo >/dev/null 2>&1; then
        echo "→ First time in container: bunny setup --minimal (several minutes)…"
        bunny setup --minimal
      fi
      [[ -f ~/.cargo/env ]] && source ~/.cargo/env
      exec bash -l
    '
    ;;
  bootstrap)
    docker compose -f "$COMPOSE_FILE" up -d
    echo "→ One-shot dev bootstrap (install + build + configure prompts)…"
    docker compose -f "$COMPOSE_FILE" exec -it bunny-dev bash -lc '
      cd /opt/bunny
      export BUNNY_DOCKER_DEV=1
      [[ -f ~/.cargo/env ]] && source ~/.cargo/env
      if ! command -v cargo >/dev/null 2>&1; then
        bunny setup --minimal
      fi
      source ~/.cargo/env 2>/dev/null || true
      bunny config-init || true
      exec bunny configure
    '
    ;;
  browser-setup)
    docker compose -f "$COMPOSE_FILE" up -d
    echo "→ Installing browser stack (Xvfb, Chromium, noVNC) — several minutes…"
    docker compose -f "$COMPOSE_FILE" exec bunny-dev bash -lc '
      set -e
      cd /opt/bunny
      ./scripts/install-prerequisites.sh
      echo ""
      echo "→ Verify:"
      bunny doctor || true
    '
    echo ""
    echo "✓ Browser tab ready — reload the Browser panel in the Web UI"
    ;;
  logs)
    docker compose -f "$COMPOSE_FILE" logs -f bunny-dev
    ;;
  status)
    docker compose -f "$COMPOSE_FILE" ps
    if curl -sf "http://127.0.0.1:7681/api/v1/agent/info" >/dev/null 2>&1; then
      echo "✓ Agent: http://127.0.0.1:7681"
    else
      echo "⚠ Agent not up — run: ./scripts/docker-dev.sh shell → bunny run"
    fi
    if [[ -f "${ROOT}/.discord/bridge.yaml" ]]; then
      echo "✓ Bridge config: .discord/bridge.yaml"
    fi
    ;;
  *)
    cat <<'EOF'
Bunny Docker dev

  ./scripts/docker-dev.sh bootstrap       RECOMMENDED: install Rust + bunny configure
  ./scripts/docker-dev.sh browser-setup   Install Xvfb + Chromium + noVNC (Browser tab)
  ./scripts/docker-dev.sh up              Start container
  ./scripts/docker-dev.sh init            Create config.yaml + hint for owner
  ./scripts/docker-dev.sh shell           Shell in container (auto setup if needed)
  ./scripts/docker-dev.sh discord-setup   Write agent + .discord/bridge.yaml (needs .discord/.env)
  ./scripts/docker-dev.sh discord-dev     One-shot setup + background agent/bridge (needs .discord/.env)
  ./scripts/docker-dev.sh start-agent     bunny run (foreground, in container)
  ./scripts/docker-dev.sh start-bridge    bunny discord bridge (foreground, in container)
  ./scripts/docker-dev.sh check-network   Test DNS vers Discord dans le conteneur
  ./scripts/docker-dev.sh agent-logs      Tail agent logs (background mode)
  ./scripts/docker-dev.sh bridge-logs     Tail discord bridge logs (background mode)
  ./scripts/docker-dev.sh status          Quick health check
  ./scripts/docker-dev.sh down            Stop container
  ./scripts/docker-dev.sh reset           Wipe Docker volume + Discord + ~/.config/bunny + build artifacts

Quick path:
  up → init → shell → bunny run
  (Mac) cp .discord/env.example .discord/.env → discord-setup → run-discord-bridge.sh
EOF
    ;;
esac
