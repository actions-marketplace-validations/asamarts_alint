#!/usr/bin/env python3
"""Summarize a same-session criterion A/B as Markdown.

bench-record benches the previous release's criterion suite with
`--save-baseline prev`, then the new ref's with `--baseline-lenient prev`.
Criterion then writes `<bench>/change/estimates.json` (relative change of
the mean and median, with a 95% confidence interval) for every benchmark
both runs share. This prints one table row per benchmark, largest median
change first, and marks a change "significant" when its confidence interval
excludes zero. Report-only: nothing here gates.

Usage: criterion-change-summary.py <criterion-dir> [--top N]
"""
import json
import sys
from pathlib import Path


def rows(root: Path):
    for est in sorted(root.glob("**/change/estimates.json")):
        bench_dir = est.parent.parent
        meta = bench_dir / "new" / "benchmark.json"
        try:
            bench_id = json.loads(meta.read_text())["full_id"]
        except (OSError, KeyError, ValueError):
            bench_id = bench_dir.relative_to(root).as_posix()
        data = json.loads(est.read_text())
        median = data["median"]
        lo = median["confidence_interval"]["lower_bound"]
        hi = median["confidence_interval"]["upper_bound"]
        yield bench_id, median["point_estimate"] * 100, lo * 100, hi * 100


def main(argv):
    if not argv or argv[0] in ("-h", "--help"):
        print(__doc__.strip())
        return 0 if argv else 2
    root = Path(argv[0])
    top = int(argv[argv.index("--top") + 1]) if "--top" in argv else None
    found = sorted(rows(root), key=lambda r: -r[1])
    if not found:
        print("_No same-session comparison: the previous release's suite did not run._")
        return 0
    significant = [r for r in found if r[2] > 0 or r[3] < 0]
    print(
        f"{len(found)} benchmarks compared in one session; "
        f"{len(significant)} with a significant median change (95% CI excludes 0).\n"
    )
    print("| Bench | Median change | 95% CI | |")
    print("|---|---:|---:|---|")
    shown = found if top is None else found[:top]
    for bench_id, point, lo, hi in shown:
        mark = "significant" if (lo > 0 or hi < 0) else ""
        print(f"| `{bench_id}` | {point:+.2f}% | [{lo:+.2f}%, {hi:+.2f}%] | {mark} |")
    if top is not None and len(found) > top:
        print(f"\n{len(found) - top} more benchmark(s) not shown.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
