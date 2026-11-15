#!/bin/bash
# Variety-matched control for the book arms.
#
# The book arm's confound: ctl (book off) plays 1.e4 in 151/151 games while var (book on) spreads
# over e4/d4/c4/Nf3, so "book ON beats book OFF" mixes gambit CONTENT with opening VARIETY.
#
# This arm holds the first move constant-but-varied for BOTH sides (the same openings file, taken
# from the book's own first-move distribution), so both sides get identical variety at ply 1. The
# only remaining difference is the book's LATER moves -- i.e. the gambit content proper.
#
# If the +28% survives here, it is the repertoire; if it collapses, it was variety.
# Usage: SEED=11 bash matchrun_bookctl.sh <name> [games]
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
echo "option ctl x_ownbook = false"
echo "option var x_ownbook = true"
echo "tc movetime=300"; echo "concurrency 4"
# the control: BOTH sides start from the same varied set of first moves
echo "openings /tmp/bmax/varopen.txt"
echo "seed ${SEED:-11}"; echo "maxplies 400"
echo "pgn $NAME.pgn"; echo "jsonl $NAME.jsonl"
echo "pair ctl var $GAMES"
} > "$D/match.cfg"
cd "$D" || exit 1
AGGRO_DEV=1 /root/aggroclone/AggroChess-brilliant/testing/arena/target/release/aggro_arena match match.cfg 2>&1 | tail -8
AGGRO_DEV=1 /root/aggroclone/AggroChess-brilliant/testing/arena/target/release/aggro_arena analyze "$NAME.jsonl" \
  --engine var --sf /usr/games/stockfish --sf-nodes 20000 --threads 3 --out "${NAME}_sac" --quiet 2>&1 | tail -1
AGGRO_DEV=1 /root/aggroclone/AggroChess-brilliant/testing/arena/target/release/aggro_arena analyze "$NAME.jsonl" \
  --engine ctl --sf /usr/games/stockfish --sf-nodes 20000 --threads 3 --out "${NAME}_ctl" --quiet 2>&1 | tail -1
echo "===== $NAME DONE ====="
