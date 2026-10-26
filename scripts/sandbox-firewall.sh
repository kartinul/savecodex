#!/usr/bin/env bash
# Block new outbound connections from the savecodex user to localhost/LAN/cloud metadata; internet stays open.
# usage: sudo ./sandbox-firewall.sh <user that runs savecodex serve>
set -euo pipefail
user=${1:?usage: $0 <user>}

iptables -A OUTPUT -m owner --uid-owner "$user" -p udp --dport 53 -j ACCEPT
iptables -A OUTPUT -m owner --uid-owner "$user" -p tcp --dport 53 -j ACCEPT
for net in 127.0.0.0/8 10.0.0.0/8 172.16.0.0/12 192.168.0.0/16 169.254.0.0/16 100.64.0.0/10; do
  iptables -A OUTPUT -m owner --uid-owner "$user" -m conntrack --ctstate NEW -d "$net" -j REJECT
done
