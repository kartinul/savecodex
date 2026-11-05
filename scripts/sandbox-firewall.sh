#!/usr/bin/env bash
# Block new outbound connections from the savecodex user to localhost/LAN/cloud metadata; internet stays open.
# Extra args are localhost ports it may still reach (e.g. a local LLM API). Safe to rerun.
# usage: sudo ./sandbox-firewall.sh <user that runs savecodex serve> [port...]
set -euo pipefail
user=${1:?usage: $0 <user> [port...]}
chain="savecodex-$user"

apply() {
  local ipt=$1 lo=$2; shift 2
  $ipt -N "$chain" 2>/dev/null || $ipt -F "$chain"
  $ipt -C OUTPUT -m owner --uid-owner "$user" -j "$chain" 2>/dev/null || $ipt -I OUTPUT 1 -m owner --uid-owner "$user" -j "$chain"
  $ipt -A "$chain" -p udp --dport 53 -j ACCEPT
  $ipt -A "$chain" -p tcp --dport 53 -j ACCEPT
  for port in "${ports[@]}"; do $ipt -A "$chain" -d "$lo" -p tcp --dport "$port" -j ACCEPT; done
  for net in "$@"; do $ipt -A "$chain" -m conntrack --ctstate NEW -d "$net" -j REJECT; done
}

ports=("${@:2}")
apply iptables 127.0.0.1 127.0.0.0/8 10.0.0.0/8 172.16.0.0/12 192.168.0.0/16 169.254.0.0/16 100.64.0.0/10
apply ip6tables ::1 ::1 fc00::/7 fe80::/10
