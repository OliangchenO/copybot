#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 {build|start|stop|restart|status}" >&2
  exit 64
}

action="${1:-}"
[[ $# -eq 1 && ( "$action" == build || "$action" == start || "$action" == stop || "$action" == restart || "$action" == status ) ]] || usage

# These are the installed WSL unit names. The execution mode comes from
# deploy/copybot2.toml, which must explicitly set bot.mode = "live".
engine=copybot-dry.service
watcher=copybot-watcher-dry.service
timers=(copybot-guardian.timer copybot-fillwatch.timer copybot-buywatch.timer)
audits=(copybot-guardian.service copybot-fillwatch.service copybot-buywatch.service)
units=("$engine" "$watcher" "${timers[@]}")

if [[ $EUID -eq 0 ]]; then
  systemctl_cmd=(systemctl)
else
  systemctl_cmd=(sudo systemctl)
fi

start_services() {
  "${systemctl_cmd[@]}" enable --now "$engine" "$watcher"
  "${systemctl_cmd[@]}" enable --now "${timers[@]}"
}

stop_services() {
  "${systemctl_cmd[@]}" disable --now "${timers[@]}"
  "${systemctl_cmd[@]}" stop "${audits[@]}"
  "${systemctl_cmd[@]}" disable --now "$watcher" "$engine"
}

case "$action" in
  build)
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    repo_dir="$(cd "$script_dir/.." && pwd)"
    if [[ -f "$HOME/.cargo/env" ]]; then
      source "$HOME/.cargo/env"
    fi
    if ! command -v cargo >/dev/null 2>&1; then
      if ! command -v curl >/dev/null 2>&1; then
        echo "cargo and curl are missing in WSL; install curl and rerun: $0 build" >&2
        exit 127
      fi
      echo "cargo not found; installing Rust with rustup in WSL..." >&2
      if ! curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal; then
        echo "Rustup installation failed; check WSL network/proxy access and rerun: $0 build" >&2
        exit 1
      fi
      source "$HOME/.cargo/env"
    fi
    cd "$repo_dir"
    cargo build --locked --release --manifest-path "$script_dir/../hot/Cargo.toml" --bin copybot-hot
    ;;
  start)
    start_services
    ;;
  stop)
    stop_services
    ;;
  restart)
    stop_services
    start_services
    ;;
  status)
    for unit in "${units[@]}"; do
      enabled=$("${systemctl_cmd[@]}" is-enabled "$unit" 2>/dev/null || true)
      if "${systemctl_cmd[@]}" is-active --quiet "$unit"; then
        printf '%-32s active   %s\n' "$unit" "$enabled"
      else
        printf '%-32s inactive %s\n' "$unit" "$enabled"
      fi
    done
    ;;
esac
