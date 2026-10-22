# Engineering reviews (Astra, read-only)

Astra = OpenAI Codex CLI 0.163.0 using `gpt-6-astra` at `model_reasoning_effort=high`, `-s read-only --ephemeral`,
through the user's existing ChatGPT login (no API key). Claude implements; Astra only reviews.

## Checkpoint A — Storage scanner (`scanner.rs`, `categories.rs`)

**Why:** P0 flagship; unsafe Win32 buffer parsing, hard-link/reparse accounting and 6M-file memory behaviour are easy to get subtly wrong.

**Question:** correctness of accounting, termination/cancellation races, buffer-parse safety, memory at 10M+ files, misleading totals.

| Recommendation | Decision |
|---|---|
| Partial enumeration errors vanish (`Ok(out)` after an error) | **Adopted** – `enumerate_dir` returns `(entries, error)`; entries kept, dir flagged `F_ERR`, error counted |
| Unaligned `&*p` + no bounds checks in the `FILE_ID_EXTD_DIR_INFO` walk | **Adopted** – 8-byte-aligned `Vec<u64>` buffer, header/name/`NextEntryOffset` bounds + alignment checks |
| Reparse policy drops data (cloud dirs, WOF/dedup files zeroed) | **Adopted** – only symlink/junction/appexeclink/AF_UNIX-style tags are links; cloud-tag dirs are followed; WOF/dedup/other data-bearing file reparse points keep reported sizes. Dirs are opened with `FILE_FLAG_OPEN_REPARSE_POINT` |
| Hard-link id fold can collide; `size > 0` filter wrong | **Adopted** – only IDs with upper 64 bits = 0 (NTFS) are used, un-folded; zero-length files with allocation included |
| Cancellation still schedules children | **Adopted** – no children queued once cancelled |
| Worker-panic hang / builder keeps `work_rx` | **Adopted partly** – builder drops its `work_rx`, disconnect ends the loop; release profile uses `panic="abort"` so a worker panic is process-fatal rather than a hang |
| `volume_space` returned caller-available free bytes | **Adopted** – uses total free bytes |
| `from_utf8_unchecked` on deserialized names | **Adopted** – checked UTF-8 |
| Lowercase allocation per name, unreachable `"resS"` | **Adopted** |
| Allocation size ≠ physical usage (compression, sparse, ADS, MFT residents) | **Documented** – Coverage tab and tooltips state the limits; not changed |
| Backpressure / chunked storage for 10M+ files | **Deferred** – ~56 B/node (≈0.4 GB for the measured 7M nodes), tree is unloaded after 5 idle minutes; documented as a known limit |

**Validation:** full C: scan before/after: 5.96M files / 973k dirs, 602 access errors, 1.61 TB found vs 1.67 TB reported used (unexplained gap shown to the user); scan takes 18–22 s warm, 78 s cold; UI stays responsive; save/load 2.5 s.

## Checkpoint B — RAM Detective (`winsys.rs`, `monitor.rs`, `recorder.rs`, `store.rs`)

**Why:** P0 — the user must trust which process is blamed for a RAM spike. Hand-written NT structure offsets, memory arithmetic and attribution logic are easy to get subtly wrong.

**Question:** struct offsets, memory composition, PID reuse, spike attribution, SQLite integrity/locking, PDH GPU parsing.

**Confirmed correct:** all `SYSTEM_PROCESS_INFORMATION` x64 offsets and the `SYSTEM_PAGEFILE_INFORMATION` layout (checked against phnt); `total - available - modified` and the five PDH memory counters; per-process CPU% maths; `(pid, create_time)` keying.

| Recommendation | Decision |
|---|---|
| Terminate: PID can be reused between cached check and `OpenProcess`; FILETIME loses precision as a JS number | **Adopted** – one handle opened, `GetProcessTimes` compared, same handle terminated; `createTime` now travels as a decimal string |
| Spike snapshots duplicate periodic rows → inflated `SUM(pws)` peaks | **Adopted** – migration removes duplicates, `UNIQUE(ts,pid,ct)` + `INSERT OR IGNORE` |
| Attribution compared a 140 s-old snapshot with a 120 s baseline; small processes treated as new; exits ignored | **Adopted** – snapshot nearest the baseline moment (±45 s), 4 MiB tracking floor, signed deltas for exited processes, "new process" counts, and an explicit *not in process memory* residual |
| Bounds-check the NT buffers, reject 32-bit builds, skip empty pagefile entries | **Adopted** |
| PDH `PDH_CSTATUS_NEW_DATA` (1) is valid | **Adopted** |
| GPU: show busiest real engine, don't zero when counters are missing | **Adopted** – per-instance max (Task Manager definition); GPU columns are `—` unless the counters returned data |
| Alert/spike sustain timers survive sampling gaps | **Adopted** – timers/rings reset after a gap |
| Peaks used the peak row's CPU/instances instead of independent maxima; growth could report exited processes | **Adopted** |
| Single mutex held during UI queries and CSV export | **Adopted** – UI queries use separate read-only WAL connections; failed flushes keep a bounded retry buffer |
| Label Memory Compression WS as an estimate | **Adopted** (tooltip) |
| Dedicated writer thread, `Result`-returning native queries, `Available Bytes` cross-check | **Deferred** – current design is correct for single-writer WAL; documented |

**Validation:** typecheck + cargo check; live app re-run (see PROGRESS.md).
