#!/bin/sh
# Generate, route and check the deck's board, then write the files for the
# board house.
#
#   KICAD=/Applications/KiCad/KiCad.app FREEROUTING=freerouting.jar JAVA=java ./route.sh
#
# Freerouting 2.4 needs Java 25. Its result varies a little from run to run,
# and once in a while it leaves a connection or two unrouted; so routing is
# tried up to three times, and the files in fab/ are written only from a
# board with no errors and nothing unconnected.
set -eu
cd "$(dirname "$0")"
KICAD=${KICAD:-/Applications/KiCad/KiCad.app}
PY="$KICAD/Contents/Frameworks/Python.framework/Versions/Current/bin/python3"
CLI="$KICAD/Contents/MacOS/kicad-cli"
JAVA=${JAVA:-java}
FREEROUTING=${FREEROUTING:-freerouting.jar}

rm -rf fab
clean=no
for attempt in 1 2 3; do
    echo "== routing, attempt $attempt"
    "$PY" generate.py "$KICAD/Contents/SharedSupport/footprints"
    "$PY" route.py export
    "$JAVA" -jar "$FREEROUTING" -de deck.dsn -do deck.ses -mp 100
    "$PY" route.py import
    rm -f deck.dsn deck.ses
    if "$CLI" pcb drc --severity-all --exit-code-violations -o drc.rpt deck.kicad_pcb; then
        clean=yes
        break
    fi
    grep -E "^\*\* Found" drc.rpt
done
if [ "$clean" != yes ]; then
    echo "no clean board after three attempts: see drc.rpt" >&2
    exit 1
fi

# Files for the board house.
mkdir -p fab/gerbers
"$CLI" pcb export gerbers --layers F.Cu,B.Cu,F.Paste,B.Paste,F.Silkscreen,B.Silkscreen,F.Mask,B.Mask,Edge.Cuts \
    --subtract-soldermask -o fab/gerbers/ deck.kicad_pcb
"$CLI" pcb export drill --format excellon --excellon-separate-th -o fab/gerbers/ deck.kicad_pcb
"$CLI" pcb export pos --side back --format csv --units mm -o fab/positions-bottom.csv deck.kicad_pcb
"$PY" fab.py
(cd fab/gerbers && zip -q ../gerbers.zip ./*)
mkdir -p renders
"$CLI" pcb render --side top --width 2000 --height 1600 --quality basic -o renders/top.png deck.kicad_pcb
"$CLI" pcb render --side bottom --width 2000 --height 1600 --quality basic -o renders/bottom.png deck.kicad_pcb
echo "== clean board; files in fab/"
