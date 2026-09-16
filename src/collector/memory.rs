//! Memory + swap metrics from `/proc/meminfo`.

use std::fs;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct MemorySample {
    pub total_mb: f64,
    /// total - available (matches what users expect "used" to mean).
    pub used_mb: f64,
    pub available_mb: f64,
    pub cached_mb: f64,
    pub buffers_mb: f64,
    pub swap_total_mb: f64,
    pub swap_used_mb: f64,
    /// Utilization fraction 0..1 for progress bars.
    pub used_fraction: f64,
    pub swap_used_fraction: f64,
}

pub fn read() -> MemorySample {
    let mut fields = std::collections::HashMap::new();
    if let Ok(content) = fs::read_to_string("/proc/meminfo") {
        for line in content.lines() {
            if let Some((k, v)) = line.split_once(':') {
                let kb: f64 = v
                    .trim()
                    .trim_end_matches("kB")
                    .trim()
                    .parse()
                    .unwrap_or(0.0);
                fields.insert(k.to_string(), kb);
            }
        }
    }
    let get = |k: &str| *fields.get(k).unwrap_or(&0.0) / 1024.0; // KiB -> MiB

    let total_mb = get("MemTotal");
    let available_mb = fields.get("MemAvailable").copied().unwrap_or(0.0) / 1024.0;
    // Fall back to MemFree if MemAvailable is missing (very old kernels).
    let available_mb = if available_mb > 0.0 {
        available_mb
    } else {
        fields.get("MemFree").copied().unwrap_or(0.0) / 1024.0
    };
    let used_mb = (total_mb - available_mb).max(0.0);
    let cached_mb = get("Cached") + get("SReclaimable");
    let swap_total_mb = get("SwapTotal");
    let swap_free_mb = get("SwapFree");
    let swap_used_mb = (swap_total_mb - swap_free_mb).max(0.0);

    MemorySample {
        total_mb,
        used_mb,
        available_mb,
        cached_mb,
        buffers_mb: get("Buffers"),
        swap_total_mb,
        swap_used_mb,
        used_fraction: if total_mb > 0.0 {
            used_mb / total_mb
        } else {
            0.0
        },
        swap_used_fraction: if swap_total_mb > 0.0 {
            swap_used_mb / swap_total_mb
        } else {
            0.0
        },
    }
}
