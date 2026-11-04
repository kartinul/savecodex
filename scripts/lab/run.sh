#!/bin/sh
# usage: run.sh <testdir> <iteration> <native|docker|container> files...   (paths relative to testdir)
# AI tier config comes from the caller's env (SMART_*/MID_*/FAST_*/GEMINI_*/SAVECODEX_*).
set -e
repo=$(cd "$(dirname "$0")/../.." && pwd)
dir=$1 n=$2 mode=$3; shift 3
out=res/iteration$n
mkdir -p "$dir/$out"
cd "$dir"
case $mode in
native) savecodex solve "$@" -o "$out/{}.docx" ;;
docker|container)
  rt=$mode
  [ $rt = container ] && { container system start --enable-kernel-install >/dev/null; trap 'container builder stop' EXIT; }
  $rt build -f "$repo/scripts/lab/Dockerfile" -t savecodex-lab "$repo" >/dev/null
  env | grep -E '^(SMART|MID|FAST|GEMINI|SAVECODEX)_' > "$out/.env.docker" || true
  $rt run --rm --env-file "$out/.env.docker" -v "$PWD:/work" savecodex-lab solve "$@" -o "$out/{}.docx"
  rm -f "$out/.env.docker" ;;
esac
