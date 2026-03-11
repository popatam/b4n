#!/usr/bin/env bash
set -euo pipefail

require_env() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    echo "Missing required env: ${name}" >&2
    exit 1
  fi
}

trim() {
  local value="$1"
  value="${value#"${value%%[![:space:]]*}"}"
  value="${value%"${value##*[![:space:]]}"}"
  printf '%s' "$value"
}

derive_local_target() {
  local bind_addr="$1"
  local host="${bind_addr%:*}"
  local port="${bind_addr##*:}"

  if [[ "$host" == "0.0.0.0" ]]; then
    printf '127.0.0.1:%s' "$port"
    return
  fi

  printf '%s' "$bind_addr"
}

require_env B4_LISTEN
require_env B4_ADMIN
require_env B4_SEED
require_env B4_VALIDATOR_PUBKEYS

LOG_DIR="${B4_LOG_DIR:-/var/log/b4}"
LOG_FILE="${B4_LOG_FILE:-${LOG_DIR}/node.log}"
WEB_HOST="${WEB_HOST:-0.0.0.0}"
WEB_PORT="${WEB_PORT:-8080}"
ADMIN_TARGET="${B4_ADMIN_TARGET:-$(derive_local_target "$B4_ADMIN")}"

mkdir -p "$LOG_DIR"
: > "$LOG_FILE"

declare -a node_args
declare -a validators
declare -a peers

node_args=(
  --listen "$B4_LISTEN"
  --admin "$B4_ADMIN"
  --seed "$B4_SEED"
)

IFS=',' read -r -a validators <<< "$B4_VALIDATOR_PUBKEYS"
validator_count=0
for raw_validator in "${validators[@]}"; do
  validator="$(trim "$raw_validator")"
  if [[ -z "$validator" ]]; then
    continue
  fi

  node_args+=(--validator-pubkey "$validator")
  ((validator_count += 1))
done

if (( validator_count == 0 )); then
  echo "B4_VALIDATOR_PUBKEYS must contain at least one value" >&2
  exit 1
fi

IFS=',' read -r -a peers <<< "${B4_PEERS:-}"
for raw_peer in "${peers[@]}"; do
  peer="$(trim "$raw_peer")"
  if [[ -z "$peer" ]]; then
    continue
  fi

  node_args+=(--peer "$peer")
done

cleanup() {
  if [[ -n "${WEB_PID:-}" ]]; then
    kill "${WEB_PID}" 2>/dev/null || true
  fi
  if [[ -n "${NODE_PID:-}" ]]; then
    kill "${NODE_PID}" 2>/dev/null || true
  fi

  wait "${WEB_PID:-}" 2>/dev/null || true
  wait "${NODE_PID:-}" 2>/dev/null || true
}

trap cleanup EXIT INT TERM

/usr/local/bin/b4n "${node_args[@]}" >> "$LOG_FILE" 2>&1 &
NODE_PID=$!

export B4_LOG_PATH="$LOG_FILE"
export B4_ADMIN_TARGET="$ADMIN_TARGET"

python3 -m uvicorn webapp.main:app --host "$WEB_HOST" --port "$WEB_PORT" &
WEB_PID=$!

wait -n "$NODE_PID" "$WEB_PID"
status=$?

cleanup
exit "$status"
