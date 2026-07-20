# det_check absolute counts — v0.13.0 vs v0.14.0

Raw Valgrind (callgrind, via gungraun) instruction and estimated-cycle counts for
the `det_check` binary bench, which runs the real release `alint check` CLI over
`gen-monorepo` synthetic trees. Deterministic: byte-identical trees (seed
`0xA11E47`), counts depend only on the binary, not on load or measurement session.

Measured separately per tag (the two tags need different `gungraun-runner`
versions — see the investigation README's "Secondary finding"):

- **v0.13.0** = `9a341559`, `gungraun-runner` 0.19.1
- **v0.14.0** = `e77a0074`, `gungraun-runner` 0.19.3

Command per checkout: `cargo bench -p alint-bench --bench det_check`.

## Instructions (Ir)

| bench | v0.13.0 | v0.14.0 | Δ% |
|---|--:|--:|--:|
| s1_1k  | 40,837,558 | 40,759,787 | −0.2% |
| s1_10k | 326,732,790 | 326,282,949 | −0.1% |
| s2_1k  | 52,110,343 | 52,299,988 | +0.4% |
| s2_10k | 383,072,389 | 382,448,978 | −0.2% |
| s6_1k  | 172,986,189 | 172,821,130 | −0.1% |
| s6_10k | 1,776,685,530 | 1,773,884,012 | −0.2% |
| s7_1k  | 68,064,424 | 67,820,154 | −0.4% |
| s7_10k | 481,823,941 | 483,447,023 | +0.3% |
| s12_1k | 64,075,993 | 64,176,464 | +0.2% |
| s12_10k | 521,195,729 | 521,515,206 | +0.1% |

## Estimated Cycles (Ir + cache + branch penalties)

| bench | v0.13.0 | v0.14.0 | Δ% |
|---|--:|--:|--:|
| s1_1k  | 59,633,786 | 59,521,116 | −0.2% |
| s1_10k | 462,509,705 | 461,993,790 | −0.1% |
| s2_1k  | 76,476,651 | 76,775,274 | +0.4% |
| s2_10k | 544,984,828 | 544,135,946 | −0.2% |
| s6_1k  | 225,645,629 | 225,400,527 | −0.1% |
| s6_10k | 2,284,490,573 | 2,280,373,761 | −0.2% |
| s7_1k  | 99,000,012 | 98,666,384 | −0.3% |
| s7_10k | 682,286,345 | 684,761,934 | +0.4% |
| s12_1k | 94,186,911 | 94,366,840 | +0.2% |
| s12_10k | 742,874,966 | 743,679,525 | +0.1% |

Every delta is within ±0.4% (measurement noise). Notably **s2_10k**, whose
wall-clock `min_ms` regressed +17.6% in the `bench-record` corpus, is −0.2% on
both instructions and estimated cycles — proof the binary does identical work and
the wall-clock delta is external to the code.
