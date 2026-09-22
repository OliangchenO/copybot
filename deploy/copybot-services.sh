#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 {start|stop|status}" >&2
  exit 64
}

action="${1:-}"
[[ $# -eq 1 && ( "$action" == start || "$action" == stop || "$action" == status ) ]] || usage

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

case "$action" in
  start)
    "${systemctl_cmd[@]}" start "$engine" "$watcher"
    "${systemctl_cmd[@]}" start "${timers[@]}"
    ;;
  stop)
    "${systemctl_cmd[@]}" stop "${timers[@]}"
    "${systemctl_cmd[@]}" stop "${audits[@]}"
    "${systemctl_cmd[@]}" stop "$watcher" "$engine"
    ;;
  status)
    for unit in "${units[@]}"; do
      if "${systemctl_cmd[@]}" is-active --quiet "$unit"; then
        printf '%-32s active\n' "$unit"
      else
        printf '%-32s inactive\n' "$unit"
      fi
    done
    ;;
esac
