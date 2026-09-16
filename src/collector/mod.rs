//! Data collection layer.
//!
//! Everything in this module reads directly from kernel interfaces
//! (`/proc`, `/sys/class/hwmon`, `/sys/class/drm`, DRM `fdinfo`) —
//! no external tools, no vendor SDKs. See the `README` for a data
//! source map.
//!
//! Design: a [`Sampler`] runs on a background thread at a fixed
//! interval and emits fully-differenced [`Snapshot`]s over a channel.
//! The UI thread never touches the filesystem.

pub mod cpu;
pub mod gpu;
pub mod hwmon;
pub mod memory;

use std::sync::mpsc::{Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

/// A single, self-consistent view of the whole machine.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    /// Milliseconds since the sampler started.
    pub uptime_ms: u128,
    pub cpu: cpu::CpuSample,
    pub memory: memory::MemorySample,
    pub gpus: Vec<gpu::GpuSample>,
    /// Chassis / motherboard fans (GPU fans live in `gpus`).
    pub system_fans: Vec<NamedFan>,
    /// Non-fatal notices, e.g. "RAPL needs root — CPU power unavailable".
    pub warnings: Vec<String>,
}

/// A fan with a friendly label, e.g. "asus: fan1".
#[derive(Debug, Clone, Serialize)]
pub struct NamedFan {
    pub label: String,
    pub rpm: f64,
}

/// Spawn the background poller. It owns previous raw counter values so
/// the snapshots it emits are already rate-converted (utilization %,
/// watts, ...). The first snapshot arrives ~`interval` after start
/// (counters need two reads to produce a delta).
pub fn spawn_sampler(interval: Duration) -> (Receiver<Snapshot>, thread::JoinHandle<()>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = thread::Builder::new()
        .name("arcanum-sampler".into())
        .spawn(move || run(tx, interval))
        .expect("failed to spawn sampler thread");
    (rx, handle)
}

fn run(tx: Sender<Snapshot>, interval: Duration) {
    let mut cpu_state = cpu::CpuState::new();
    let mut gpu_state = gpu::GpuState::new();
    let mut rapl_state = cpu::RaplState::new();
    let start = Instant::now();

    // First pass: prime counters without emitting (most deltas are
    // undefined with no previous sample).
    {
        let _ = gpu_state.sample();
        let _ = cpu_state.sample();
        let _ = rapl_state.sample();
    }

    loop {
        thread::sleep(interval);

        let mut warnings = Vec::new();
        let cpu = cpu_state.sample();
        let memory = memory::read();
        let gpus = gpu_state.sample();
        warnings.extend(gpu_state.take_warnings());

        // CPU package power via RAPL (needs root; degrades gracefully).
        let (pkg_power, rapl_denied) = rapl_state.sample();
        if rapl_denied {
            warnings.push(
                "CPU package power unavailable: RAPL energy counters require root. \
                 See scripts/setup-permissions.sh"
                    .into(),
            );
        }
        let cpu = cpu::CpuSample {
            package_power_w: pkg_power,
            ..cpu
        };

        let system_fans = gpu_state.system_fans();

        let snapshot = Snapshot {
            uptime_ms: start.elapsed().as_millis(),
            cpu,
            memory,
            gpus,
            system_fans,
            warnings,
        };

        if tx.send(snapshot).is_err() {
            return; // UI went away — stop polling.
        }
    }
}
