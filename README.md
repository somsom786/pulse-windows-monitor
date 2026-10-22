# PULSE — local Windows system intelligence

Tauri 2 (Rust) + React/TypeScript/Tailwind + ECharts. Everything is local: no network calls, accounts or telemetry.

## License
MIT. Forks and contributions welcome.

## Run / build
```powershell
npm install
npm run tauri dev        # development (Rust + Vite hot reload)
npm run tauri build      # release exe + NSIS installer
```
Outputs: `src-tauri/target/release/pulse.exe` and `src-tauri/target/release/bundle/nsis/PULSE_0.1.0_x64-setup.exe`.
Requires Node 20+, Rust (MSVC toolchain), WebView2 (included in Windows 11).
Checks: `npm run typecheck`, `cargo check` in `src-tauri`. Dev examples: `cargo run --release --example scantest -- C:\`, `--example teltest`.

## What it does
- **Storage Explorer** – parallel NTFS scanner (≈20 s warm / 78 s cold for 6 M files), zoomable treemap, allocated vs logical size, filters, largest files/folders, file types, cleanup locations, installed apps, old installers, duplicate finder, growth between saved scans, coverage report. Actions: Explorer, properties, copy path, rescan, Recycle Bin (confirmed, protected paths refused). Scans are saved in `%APPDATA%\PULSE\scans`.
- **Processes & Memory** – Windows memory composition (in use / modified / standby / free, commit, pagefile, kernel pools), live process table (private WS, WS, commit, CPU, GPU %, VRAM, I/O, grouping, icons), spike investigation (click the history chart → processes at that moment, growth attribution with an "unexplained" residual, peaks, sustained-growth observations), guarded process termination.
- **Overview, Hardware, History, Settings** – live cards, "Needs attention" (measured facts only), NVML GPU telemetry, drives/adapters, sensor status table, CSV export, alerts with cooldowns, tray, launch at sign-in.

## Performance (release build, this PC, measured)
- `pulse.exe` (sampler + recorder): ~60 MB working set / 32 MB private, ~1% of one core, one sample ≈ 25 ms every 2 s.
- With the window open the WebView2 tree adds ~300–400 MB private and 1–5% of one core (Overview is the heaviest page).
- Closing the window to the tray **destroys the WebView**; monitoring and recording continue at ~60 MB.
- Full-drive scan tree: ~56 B/node (≈0.4 GB for 7 M nodes) in RAM only while in use; unloaded after 5 idle minutes. History DB is bounded by retention (default 7 days).

## Limitations / unsupported
- CPU/motherboard temperatures, fan RPM and CPU power need a kernel driver. PULSE reads them from **LibreHardwareMonitor** when it runs as administrator with *Options → Remote Web Server → Run* (loopback :8085). This integration is implemented but **not yet verified against live LHM data** on this PC (it needs a UAC approval). Otherwise these show "Needs elevation / driver".
- GPU hotspot temperature is not exposed by NVML for GeForce. Per-process network usage needs elevated ETW and is not collected.
- Process "I/O rate" is Windows I/O counters (files, pipes, network), not only disk.
- Directory-reported allocation can differ from physical use for compressed/sparse/deduplicated files; MFT-resident files and alternate streams are not itemised.
- Unelevated scans skip folders Windows denies (reported in the Coverage tab).

See `PROGRESS.md` and `ENGINEERING_REVIEWS.md`.
