#!/usr/bin/env bash
# Core crates: median line coverage ≥ 93%.
# UI/layout crates (scene/render): each ≥ 80% (large canvas/layout surface).
set -euo pipefail

CORE=(
  architect-c4-domain
  architect-c4-revision
  architect-c4-session
  architect-c4-model
  architect-c4-git
  architect-c4-adr
  architect-c4-flow
  architect-c4-queue
  architect-c4-tomlio
  architect-c4-policy
  architect-c4-validate
)
UI=(
  architect-c4-scene
  architect-c4-render
)

line_pct() {
  local c=$1
  local summary
  summary=$(cargo llvm-cov -p "$c" --summary-only 2>/dev/null || true)
  printf '%s\n' "$summary" | python3 -c '
import sys,re
text=sys.stdin.read()
last=""
for line in text.splitlines():
    if line.strip().startswith("TOTAL"):
        pcts=re.findall(r"([0-9]+(?:\.[0-9]+)?)%", line)
        if len(pcts) >= 3:
            last=pcts[2]
print(last)
'
}

core_pcts=()
for c in "${CORE[@]}"; do
  line=$(line_pct "$c")
  if [[ -z "${line}" ]]; then
    echo "FAIL: no coverage for $c" >&2
    exit 1
  fi
  echo "$c ${line}%"
  core_pcts+=("$line")
done

ui_pcts=()
for c in "${UI[@]}"; do
  line=$(line_pct "$c")
  if [[ -z "${line}" ]]; then
    echo "FAIL: no coverage for $c" >&2
    exit 1
  fi
  echo "$c ${line}% (ui)"
  ui_pcts+=("$line")
done

python3 - <<'PY' "${core_pcts[@]}" -- "${ui_pcts[@]}"
import sys
args=sys.argv[1:]
sep=args.index("--")
core=sorted(float(x) for x in args[:sep])
ui=[float(x) for x in args[sep+1:]]
n=len(core)
med = core[n//2] if n%2 else (core[n//2-1]+core[n//2])/2
print(f"core_median_line_coverage={med:.2f}%")
print("core_sorted=", core)
print("ui=", ui)
if med < 93.0:
    raise SystemExit(f"FAIL: core median {med:.2f}% < 93%")
if any(u < 80.0 for u in ui):
    raise SystemExit(f"FAIL: UI crate below 80%: {ui}")
print("OK")
PY
