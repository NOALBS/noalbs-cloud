#!/bin/sh
set -euo pipefail

declare -a PIDS=()

start_service() {
  local name="$1"
  shift

  echo "starting $name..."
  "$@" &
  PIDS+=("$!")
}

cleanup() {
  trap - INT TERM EXIT
  echo
  echo "stopping services..."
  for pid in "${PIDS[@]}"; do
    if kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
    fi
  done
  wait "${PIDS[@]}" 2>/dev/null || true
}

trap cleanup INT TERM EXIT

# start_service "NATS Server" nats-server -js
start_service "Simple HTTP Server" simple-http-server

echo "all services started. press Ctrl+C to stop."

set +e
wait -n "${PIDS[@]}"
status=$?
set -e

if [[ $status -ne 0 ]]; then
  echo "a service exited with status $status; shutting down the rest..."
else
  echo "a service exited; shutting down the rest..."
fi

exit "$status"