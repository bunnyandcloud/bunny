#!/usr/bin/env bash
# Ensure `bunny` is on PATH for every shell/exec in the dev container.
set -euo pipefail

BUNNY_ROOT="/opt/bunny"
if [[ -x "${BUNNY_ROOT}/bunny" ]]; then
  ln -sf "${BUNNY_ROOT}/bunny" /usr/local/bin/bunny
fi

cat >/etc/profile.d/bunny-dev.sh <<'EOF'
export LANG=C.UTF-8
export LC_ALL=C.UTF-8
export PATH="/opt/bunny:/root/.cargo/bin:/usr/local/bin:${PATH}"
[[ -f /root/.cargo/env ]] && source /root/.cargo/env
EOF

export LANG=C.UTF-8
export LC_ALL=C.UTF-8
export PATH="/opt/bunny:/root/.cargo/bin:/usr/local/bin:${PATH}"
[[ -f /root/.cargo/env ]] && source /root/.cargo/env

exec "$@"
