#!/usr/bin/env python3
"""Count read- and stat-family system calls in an `strace -f` log.

Only the traced process and its threads count. A pid that calls `execve` after
the first one is a spawned child (`git`, `true`), whose calls vary by host, so
its lines are skipped. The resumed half of an interrupted call (`<... read
resumed>`) was already counted where it started.

    syscall-counts.py --syscalls   print the strace `-e trace=` list
    syscall-counts.py LOG          print `family count` lines for LOG

Used by reference-perf-gate.sh (ADR-0023, V21-6).
"""

import re
import sys

FAMILIES = {
    "read": {"read", "pread64", "readv"},
    "stat": {
        "newfstatat", "fstat", "statx", "lstat", "stat",
        "access", "faccessat2", "readlink", "readlinkat",
    },
    "open": {"openat"},
    "dents": {"getdents64"},
}
TRACED = sorted(set().union(*FAMILIES.values()) | {"execve"})


def count(lines):
    rows = [line.rstrip("\n") for line in lines if re.match(r"^\d+ ", line)]
    totals = {family: 0 for family in FAMILIES}
    if not rows:
        return totals
    main = rows[0].split(" ", 1)[0]
    children = {
        row.split(" ", 1)[0]
        for row in rows
        if " execve(" in row and row.split(" ", 1)[0] != main
    }
    for row in rows:
        pid, rest = row.split(" ", 1)
        rest = rest.lstrip()
        if pid in children or rest.startswith("<..."):
            continue
        call = re.match(r"(\w+)\(", rest)
        if not call:
            continue
        for family, names in FAMILIES.items():
            if call.group(1) in names:
                totals[family] += 1
    return totals


def main(argv):
    if argv == ["--syscalls"]:
        print(",".join(TRACED))
        return 0
    if len(argv) != 1:
        print(__doc__, file=sys.stderr)
        return 2
    with open(argv[0], encoding="utf-8", errors="replace") as log:
        for family, total in count(log).items():
            print(family, total)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
