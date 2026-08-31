//! Standalone reproduction of the v0.14 content-read wall-clock regression and its
//! fix, isolating the exact `std` behaviour behind it. NOT part of the alint build —
//! compile with `rustc -O readrepro.rs -o readrepro` and run per this directory's
//! README.md.
//!
//! Four read paths over the same tree of small files, chosen to mirror the hot path
//! that `c845f7d3` (the v0.14 OOM cap) changed:
//!
//!   v13         `std::fs::read(p)`                    — File-specialized: fstats,
//!                                                        preallocates, reads once.
//!   v14         `File::open` + `.take(cap+1)          — what v0.14 ships. A
//!               .read_to_end(Vec::new())`               `Take<File>` has NO
//!                                                        read_to_end preallocation
//!                                                        specialization → grows and
//!                                                        re-reads → extra `read()`
//!                                                        syscalls per file.
//!   v14fix      like v14 but preallocates via a        — recovers most of it, but
//!               separate `std::fs::metadata` stat       pays an extra path stat.
//!   v14fixfree  like v14 but preallocates from a        — the SHIPPED fix: alint
//!               size collected up front (the walk        already has FileEntry::size
//!               already has it, so it is free here)      from the walk, zero extra
//!                                                        syscall. Beats v13.
//!
//! The `take(cap+1)` bound is retained in every v14* path — it is the TOCTOU/OOM
//! guard the OOM cap added; the fix keeps it and only sizes the buffer.

use std::fs::File;
use std::io::Read;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = &args[1];
    let mode = &args[2];
    let cap: u64 = 256 * 1024 * 1024; // MAX_ANALYZE_BYTES

    // Mimic the walk: collect (path, size) ONCE, up front. In alint this size rides
    // along on FileEntry from the directory walk, so in the hot read loop it costs
    // nothing — which is why `v14fixfree` is the realistic model of the fix.
    let mut items: Vec<(std::path::PathBuf, u64)> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .map(|p| {
            let sz = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            (p, sz)
        })
        .collect();
    items.sort();

    let mut total = 0usize;
    for _ in 0..5 {
        for (p, walk_size) in &items {
            let buf: Vec<u8> = match mode.as_str() {
                "v13" => std::fs::read(p).unwrap(),
                "v14" => {
                    let f = File::open(p).unwrap();
                    let mut b = Vec::new();
                    f.take(cap + 1).read_to_end(&mut b).unwrap();
                    b
                }
                "v14fix" => {
                    let sz = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                    let f = File::open(p).unwrap();
                    let mut b = Vec::with_capacity(sz.min(cap + 1) as usize);
                    f.take(cap + 1).read_to_end(&mut b).unwrap();
                    b
                }
                "v14fixfree" => {
                    let f = File::open(p).unwrap();
                    let mut b = Vec::with_capacity((*walk_size).min(cap + 1) as usize);
                    f.take(cap + 1).read_to_end(&mut b).unwrap();
                    b
                }
                _ => panic!("mode must be v13 | v14 | v14fix | v14fixfree"),
            };
            total += buf.len();
        }
    }
    eprintln!("mode={} files={} total_bytes={}", mode, items.len(), total);
}
