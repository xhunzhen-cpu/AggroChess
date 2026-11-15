#!/bin/bash
# Book arm: NO `openings` line, so games start at MOVE 1 and the curated 323-line gambit book
# actually fires. Every normal match forces the first 8 plies from 8moves_v3.txt, which is exactly
# where a gambit repertoire's value lives, so the book is invisible to every other arm.
#
# ctl = book OFF (OwnBook false), var = book ON. Same binary both sides.
# Usage: SEED=11 bash matchrun_book1.sh <name> [games]
set -u
NAME=$1; GAMES=${2:-1500}
D=/tmp/bmax/runs/$NAME; mkdir -p "$D"
if [ "${FORCE:-0}" != "1" ] && [ -f "$D/$NAME.jsonl.summary.json" ] \
   && grep -q '"complete": true' "$D/$NAME.jsonl.summary.json"; then
  echo "SKIP $NAME: already complete"; exit 0
fi
ENG=${BASEBIN:-/tmp/bmax/ctl_540behav}
cp "$ENG" "$D/ctl"; cp "$ENG" "$D/var"
{
echo "event $NAME"
echo "engine ctl $D/ctl"; echo "engine var $D/var"
for k in "Threads=1" "Hash=64" "Skill Level=21" "Brilliance=100" \
         "Sacrifice Appetite=95" "Aggression=90" "King Attack Priority=90" \
         "Material Preservation=15" "Speculative Sacrifices=90" "Human Style=70" "Human Think Time=0"; do
  echo "option ctl ${k%%=*} = ${k#*=}"; echo "option var ${k%%=*} = ${k#*=}"
done
# the one difference: the book (dev knob name is x_ownbook, not OwnBook; AGGRO_DEV is set below)
echo "option ctl x_ownbook = false"
echo "option var x_ownbook = true"
echo "tc movetime=300"; echo "concurrency 4"
# NO openings line: play from move 1
echo "seed ${SEED:-11}"; echo "maxplies 400"
echo "pgn $NAME.pgn"; echo "jsonl $NAME.jsonl"
echo "pair ctl var $GAMES"
} > "$D/match.cfg"
cd "$D" || exit 1
AGGRO_DEV=1 /root/aggroclone/AggroChess-brilliant/testing/arena/target/release/aggro_arena match match.cfg 2>&1 | tail -8
AGGRO_DEV=1 /root/aggroclone/AggroChess-brilliant/testing/arena/target/release/aggro_arena analyze "$NAME.jsonl" \
  --engine var --sf /usr/games/stockfish --sf-nodes 20000 --threads 3 --out "${NAME}_sac" --quiet 2>&1 | tail -2
echo "===== $NAME DONE ====="
