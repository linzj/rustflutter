#!/bin/sh
# Compare a chain's stub set against a baseline, and refuse to answer when
# the report is not there.
#
#   bin/stubdiff.sh <baseline.txt> <report.txt.detail.txt>
#
# Written because an absent or half-written detail file makes `comm` print
# "gone: N  new: 0", which reads exactly like a clean run. That misreading
# happened three times in one session; the guard is one line and the
# mistake is expensive.
set -u
base=$1
report=$2
if [ ! -s "$report" ]; then
    echo "NO REPORT at $report -- the chain has not finished, or it failed"
    exit 2
fi
have=$(mktemp)
grep '^=== ' "$report" | sort > "$have"
echo "stubs: $(wc -l < "$have")"
echo "gone:  $(comm -23 "$base" "$have" | wc -l)"
echo "new:   $(comm -13 "$base" "$have" | wc -l)"
comm -13 "$base" "$have" | sed 's/^=== /  + /' | head -20
rm -f "$have"
