//! CPU utilization, clocks, temperatures and package power.
//!
//! Data sources:
//! - `/proc/stat` — per-core busy/idle jiffies → utilization
//! - `.../cpuN/cpufreq/scaling_cur_freq` — per-core current clock (kHz)
//! - `coretemp` hwmon — package + per-core temperatures
//! - RAPL `powercap` sysfs — package energy (root-only; optional)

use std::fs;
use std::time::Instant;

use serde::Serialize;

use super::hwmon;

/// Per-poll CPU metrics. All rate metrics are differenced by the sampler.
#[derive(Debug, Clone, Serialize)]
pub struct CpuSample {
    pub core_count: usize,
    /// 0..100 aggregate utilization across all logical cores.
    pub utilization_pct: f64,
    /// One entry per logical core, 0..100.
    pub per_core_utilization_pct: Vec<f64>,
    /// Current per-core clock in MHz (index-aligned with utilization).
    pub per_core_mhz: Vec<f64>,
    /// Advertised base clock in MHz, if the kernel exposes it.
    pub base_mhz: Option<f64>,
    /// Package temperature (coretemp "Package id N").
    pub package_celsius: Option<f64>,
    /// Per-core temperatures, aligned with per-core utilization where
    /// labels are "Core N"; otherwise a flat list of sensor readings.
    pub per_core_celsius: Vec<f64>,
    /// Hottest core temperature.
    pub hottest_core_celsius: Option<f64>,
    /// CPU package power draw in watts (RAPL; needs root, may be None).
    pub package_power_w: Option<f64>,
}

/// Stateful differencer for /proc/stat.
pub struct CpuState {
    /// Jiffie counters at the last poll: index 0 is the `cpu` aggregate,
    /// indexes 1..=n are cpu0..cpuN-1. (totals, idles).
    prev: Option<(Vec<u64>, Vec<u64>)>,
}

impl CpuState {
    pub fn new() -> Self {
        Self { prev: None }
    }

    pub fn sample(&mut self) -> CpuSample {
        let (total_aggregate, idle_aggregate, per_core) = read_proc_stat();

        let core_count = per_core.len();
        let mut utilization_pct = 0.0;
        let mut per_core_utilization_pct = vec![0.0; core_count];

        if let Some((prev_total, prev_idle)) = &self.prev {
            utilization_pct =
                diff_pct(total_aggregate, idle_aggregate, prev_total[0], prev_idle[0]);
            for i in 0..core_count {
                per_core_utilization_pct[i] = diff_pct(
                    per_core[i].0,
                    per_core[i].1,
                    prev_total[i + 1],
                    prev_idle[i + 1],
                );
            }
        }
        let mut totals = Vec::with_capacity(core_count + 1);
        let mut idles = Vec::with_capacity(core_count + 1);
        totals.push(total_aggregate);
        idles.push(idle_aggregate);
        for (t, i) in &per_core {
            totals.push(*t);
            idles.push(*i);
        }
        self.prev = Some((totals, idles));

        let per_core_mhz = per_core_clocks(core_count);
        let base_mhz = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/base_frequency")
            .ok()
            .and_then(|s| s.trim().parse::<f64>().ok())
            .map(|khz| khz / 1000.0);

        let (package_celsius, per_core_celsius, hottest_core_celsius) = coretemps();

        CpuSample {
            core_count,
            utilization_pct,
            per_core_utilization_pct,
            per_core_mhz,
            base_mhz,
            package_celsius,
            per_core_celsius,
            hottest_core_celsius,
            package_power_w: None, // filled in by the sampler from RAPL
        }
    }
}

/// RAPL package-energy differencer.
pub struct RaplState {
    prev: Option<(u64, Instant)>,
    denied: bool,
}

impl RaplState {
    pub fn new() -> Self {
        Self {
            prev: None,
            denied: false,
        }
    }

    /// Returns (watts, permission_denied).
    pub fn sample(&mut self) -> (Option<f64>, bool) {
        let path = "/sys/class/powercap/intel-rapl:0/energy_uj";
        let Ok(raw) = fs::read_to_string(path) else {
            // Distinguish "not present" from "not permitted" only once.
            if !self.denied && !std::path::Path::new(path).exists() {
                return (None, false); // no RAPL on this machine
            }
            self.denied = true;
            return (None, true);
        };
        let Ok(energy_uj) = raw.trim().parse::<u64>() else {
            return (None, false);
        };
        let now = Instant::now();
        let result = match self.prev {
            Some((prev_uj, prev_t)) if energy_uj >= prev_uj => {
                let du = energy_uj - prev_uj;
                let dt = now.duration_since(prev_t).as_secs_f64();
                if dt > 0.0 {
                    Some(du as f64 / 1e6 / dt)
                } else {
                    None
                }
            }
            _ => None, // counter wrapped or first poll
        };
        self.prev = Some((energy_uj, now));
        self.denied = false;
        (result, false)
    }
}

/// Parse /proc/stat. Returns (cpu_total, cpu_idle, per-core (total, idle)).
fn read_proc_stat() -> (u64, u64, Vec<(u64, u64)>) {
    let mut total = 0;
    let mut idle = 0;
    let mut per_core = Vec::new();
    if let Ok(content) = fs::read_to_string("/proc/stat") {
        for line in content.lines() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let parse =
                |i: usize| -> u64 { fields.get(i).and_then(|f| f.parse().ok()).unwrap_or(0) };
            if line.starts_with("cpu ") {
                total = (1..fields.len()).map(parse).sum();
                idle = parse(4) + parse(5); // idle + iowait
            } else if line.starts_with("cpu") && fields.len() > 4 {
                let t: u64 = (1..fields.len()).map(parse).sum();
                let i = parse(4) + parse(5);
                per_core.push((t, i));
            }
        }
    }
    (total, idle, per_core)
}

fn diff_pct(total: u64, idle: u64, prev_total: u64, prev_idle: u64) -> f64 {
    let dt = total.saturating_sub(prev_total) as f64;
    let di = idle.saturating_sub(prev_idle) as f64;
    if dt > 0.0 {
        ((dt - di) / dt * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    }
}

fn per_core_clocks(core_count: usize) -> Vec<f64> {
    (0..core_count)
        .map(|i| {
            let p = format!("/sys/devices/system/cpu/cpu{i}/cpufreq/scaling_cur_freq");
            fs::read_to_string(&p)
                .ok()
                .and_then(|s| s.trim().parse::<f64>().ok())
                .map(|khz| khz / 1000.0)
                .unwrap_or(0.0)
        })
        .collect()
}

/// Extract package + per-core temps from the coretemp hwmon device.
fn coretemps() -> (Option<f64>, Vec<f64>, Option<f64>) {
    let mut package = None;
    let mut cores = Vec::new();
    for dir in hwmon::all() {
        if hwmon::name(&dir) != "coretemp" {
            continue;
        }
        for t in hwmon::readout(&dir).temps {
            if t.label.starts_with("Package") {
                package = Some(t.celsius);
            } else {
                cores.push(t.celsius);
            }
        }
    }
    let hottest = cores
        .iter()
        .cloned()
        .fold(None::<f64>, |acc, c| Some(acc.map_or(c, |a: f64| a.max(c))));
    (package, cores, hottest)
}
