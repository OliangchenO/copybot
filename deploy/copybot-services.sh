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

check_start_config() {
  local script_dir
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  [[ -x "$script_dir/../hot/target/release/copybot-hot" ]] || {
    echo "copybot binary is missing; run: $0 build" >&2
    return 1
  }
  python3 - "$script_dir/copybot2.toml" "$script_dir/copybot.env" <<'PY'
import sys
import tomllib
from pathlib import Path

config = tomllib.loads(Path(sys.argv[1]).read_text())
if config.get("bot", {}).get("mode") != "live":
    sys.exit("CONFIG ERROR: bot.mode must be live")

env = {}
for line in Path(sys.argv[2]).read_text().splitlines():
    if "=" in line and not line.lstrip().startswith("#"):
        key, value = line.split("=", 1)
        env[key.strip()] = value.strip().strip("\"'")

names = set()
for feed in config.get("feed", []):
    name = feed["name"]
    if name in names:
        sys.exit(f"CONFIG ERROR: duplicate feed name {name}")
    names.add(name)
    url, url_env = feed.get("url", ""), feed.get("url_env")
    if bool(url) == bool(url_env):
        sys.exit(f"CONFIG ERROR: feed {name}: set exactly one of url or url_env")
    if url_env:
        url = env.get(url_env, "")
    if not url.startswith("wss://"):
        sys.exit(f"CONFIG ERROR: feed {name}: expected a wss:// URL ({url_env or 'url'})")
PY
}

start_services() {
  "${systemctl_cmd[@]}" enable --now "$engine" "$watcher"
  "${systemctl_cmd[@]}" enable --now "${timers[@]}"
  local engine_pid last_pid=0 stable_since=0 deadline=$((SECONDS + 90))
  while (( SECONDS < deadline )); do
    engine_pid=$(systemctl show "$engine" --property=MainPID --value)
    if [[ "$engine_pid" != 0 ]] && systemctl is-active --quiet "$engine"; then
      if [[ "$engine_pid" != "$last_pid" ]]; then
        last_pid="$engine_pid"
        stable_since=$SECONDS
      elif (( SECONDS - stable_since >= 20 )); then
        break
      fi
    else
      last_pid=0
    fi
    sleep 2
  done
  if [[ "$last_pid" == 0 || "$last_pid" != "$(systemctl show "$engine" --property=MainPID --value)" ]] || (( SECONDS - stable_since < 20 )); then
    echo "copybot engine did not stay running for 20 seconds; inspect: sudo journalctl -u $engine -n 30" >&2
    return 1
  fi
  for unit in "${units[@]}"; do
    if ! systemctl is-active --quiet "$unit"; then
      echo "$unit is not active after start; inspect: sudo journalctl -u $unit -n 30" >&2
      return 1
    fi
  done
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
    if [[ -n "${COPYBOT_CARGO_HTTP_PROXY:-}" ]]; then
      export HTTP_PROXY="$COPYBOT_CARGO_HTTP_PROXY"
      export HTTPS_PROXY="$COPYBOT_CARGO_HTTP_PROXY"
      export http_proxy="$COPYBOT_CARGO_HTTP_PROXY"
      export https_proxy="$COPYBOT_CARGO_HTTP_PROXY"
    fi
    cd "$repo_dir"
    if ! cargo build --locked --release --manifest-path "$script_dir/../hot/Cargo.toml" --bin copybot-hot; then
      cat >&2 <<'EOF'
Build failed. If Cargo reports crates.io timeouts and WSL reports a localhost
proxy warning, pass a proxy address reachable from WSL (enable LAN access first):
  COPYBOT_CARGO_HTTP_PROXY=http://<windows-host-ip>:<port> bash deploy/copybot-services.sh build
EOF
      exit 1
    fi
    ;;
  start)
    check_start_config
    start_services
    ;;
  stop)
    stop_services
    ;;
  restart)
    check_start_config
    stop_services
    start_services
    ;;
  status)
    for unit in "${units[@]}"; do
      enabled=$(systemctl is-enabled "$unit" 2>/dev/null || true)
      if systemctl is-active --quiet "$unit"; then
        printf '%-32s active   %s\n' "$unit" "$enabled"
      else
        printf '%-32s inactive %s\n' "$unit" "$enabled"
      fi
    done
    ;;
esac
