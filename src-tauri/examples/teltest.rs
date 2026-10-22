use pulse_lib::monitor::Collector;
fn main() {
    let mut c = Collector::new();
    let _ = c.sample(2000, true);
    std::thread::sleep(std::time::Duration::from_secs(2));
    let (s, mut p) = c.sample(2000, true);
    println!("cpu {} logical={} physical={} total={:.1}% freq={:?}/{:?}", s.cpu.name, s.cpu.logical, s.cpu.physical, s.cpu.total, s.cpu.freq_mhz, s.cpu.base_mhz);
    let m = &s.mem;
    let g = |b: u64| b as f64 / 1073741824.0;
    println!("mem total={:.1} avail={:.1} inuse={:.1} modified={:?} standby={:?} free={:?} compressed={:?}", g(m.total), g(m.available), g(m.in_use), m.modified.map(g), m.standby.map(g), m.free.map(g), m.compressed.map(g));
    println!("commit {:.1}/{:.1} paged={:.2} nonpaged={:.2} pf={:?} pressure={} ({})", g(m.commit_total), g(m.commit_limit), g(m.kernel_paged), g(m.kernel_nonpaged), m.pagefiles, m.pressure, m.pressure_reason);
    println!("gpu: {:?} status={}", s.gpu.as_ref().map(|x| (x.name.clone(), x.util.value, x.vram_used.value, x.temp.value, x.fan_pct.value, x.fan_rpm.status, x.power_w.value)), s.gpu_status);
    println!("disks: {:?}", s.disks.iter().map(|d| (d.letter.clone(), d.free / 1_000_000_000, d.read_bps, d.active_pct)).collect::<Vec<_>>());
    println!("net rx={:?} tx={:?} adapters={:?}", s.net.rx_bps, s.net.tx_bps, s.net.adapters.iter().map(|a| a.name.clone()).collect::<Vec<_>>());
    println!("sample cost {:.1} ms; processes {}", s.sample_cost_ms, p.len());
    p.sort_by(|a, b| b.private_ws.cmp(&a.private_ws));
    for r in p.iter().take(8) { println!("{:>6} {:<28} pws={:>8.0}MB ws={:>8.0}MB commit={:>8.0}MB cpu={:>5.1} gpuMem={:?} path={}", r.pid, r.name, r.private_ws as f64/1048576.0, r.working_set as f64/1048576.0, r.commit as f64/1048576.0, r.cpu, r.gpu_mem.map(|x| x/1048576), r.path); }
    println!("thermal zones {:?}", c.thermal_zones);
}
