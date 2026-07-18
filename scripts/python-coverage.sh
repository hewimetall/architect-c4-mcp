#!/usr/bin/env bash
# Python coverage gate: MEDIAN of per-file line % must be ≥ FAIL_UNDER.
# Separate from Rust coverage (scripts/rust-coverage.sh). Matches agent-lsp.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FAIL_UNDER="${PY_COV_FAIL_UNDER:-93}"
cd "$ROOT"

echo "==> python coverage (median ≥ ${FAIL_UNDER}%)"
uv run pytest -q \
  --cov=architect_c4 \
  --cov-report=json:coverage-py.json \
  --cov-report=term-missing \
  --cov-fail-under=0

uv run python - <<'PY' "$FAIL_UNDER"
import json
import statistics
import sys
from pathlib import Path

fail = float(sys.argv[1])
data = json.loads(Path("coverage-py.json").read_text())
files = data.get("files") or {}
pcts: list[float] = []
for path, meta in sorted(files.items()):
    summary = meta.get("summary") or {}
    if "percent_covered" in summary:
        pcts.append(float(summary["percent_covered"]))
        continue
    num = float(summary.get("num_statements") or 0)
    covered = float(summary.get("covered_lines") or 0)
    pcts.append(100.0 if num == 0 else 100.0 * covered / num)

if not pcts:
    print("FAIL: no Python coverage files", file=sys.stderr)
    sys.exit(1)

median = statistics.median(pcts)
mean = statistics.fmean(pcts)
print(f"python files={len(pcts)} median={median:.2f}% mean={mean:.2f}% (gate=median)")
for path, meta in sorted(files.items()):
    s = meta.get("summary") or {}
    pct = s.get("percent_covered")
    if pct is None:
        num = float(s.get("num_statements") or 0)
        covered = float(s.get("covered_lines") or 0)
        pct = 100.0 if num == 0 else 100.0 * covered / num
    print(f"  {pct:6.2f}%  {path}")

if median + 1e-9 < fail:
    print(f"FAIL: python median coverage {median:.2f}% < {fail:.0f}%", file=sys.stderr)
    sys.exit(1)
print(f"OK: python median {median:.2f}% ≥ {fail:.0f}%")
PY
