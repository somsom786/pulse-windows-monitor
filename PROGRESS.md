# PULSE — progress

Authoritative status. Updated as work lands.

## Phases
| Phase | Scope | Status |
|---|---|---|
| 1 | Storage Explorer (scanner, treemap, table, filters, insights, Explorer actions) | **Done**, verified on real C: drive |
| 2 | RAM Detective (memory breakdown, live processes, history, spikes, attribution) | **Done**, verified live; Astra review B adopted |
| 3 | Overview | **Done** |
| 4 | Hardware (CPU/GPU/NVML, drives, adapters, sensor status) | **Done**; CPU temp needs LibreHardwareMonitor |
| 5 | Polish: settings, tray, autostart, alerts, CSV export | **Done** (see "Remaining") |

## Verified against real data (a Ryzen 7 7800X3D / RTX 4070 / 64 GB / 2 TB NVMe test machine)
- Full-drive scan: 5.96 M files, 973 k folders, 18–22 s warm; treemap, drill-down, filters, table render real sizes.
- Memory: in-use + modified + standby + free = total (63.1 GiB); commit/pagefile/kernel pools; per-process private WS / WS / commit.
- Real memory spikes were detected and attributed to the growing process (e.g. "RAM rose 4.2 GB in 2 min … node.exe +3.5 GB").
- NVML: utilization, temperature, VRAM, power, clocks, fan; unsupported fields (hotspot) labelled.

## Known limitations
- CPU / motherboard temperatures, fan RPM and package power need a kernel driver; PULSE reads them from LibreHardwareMonitor's WMI provider when it is running, else shows "Needs elevation / driver".
- GPU hotspot temperature is not exposed by NVML on GeForce.
- Per-process network usage not available (needs elevated ETW).
- Process I/O rate is Windows I/O counters (files + pipes + network), not strictly disk.
- Directory-reported allocation can differ from physical usage for compressed / sparse / dedup'd files; MFT-resident files and ADS are not itemised.

## Remaining / ideas
- LibreHardwareMonitor web-server parsing untested against live data (needs UAC).
- Installer built: src-tauri/target/release/bundle/nsis/PULSE_0.1.0_x64-setup.exe (unsigned).
