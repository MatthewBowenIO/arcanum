//! Generic `hwmon` sysfs helpers.
//!
//! Kernel sensors (temps, fans, power, energy) are exposed as
//! `/sys/class/hwmon/hwmon*` directories with a `name` file and
//! numbered attribute files (`temp1_input`, `fan1_input`, ...).
//! GPU drivers create hwmon entries too — `i915` and `xe` each have
//! one — which is where GPU temps, fans and energy counters live.

use std::fs;
use std::path::{Path, PathBuf};

/// One numbered temperature sensor, e.g. `("pkg", 49.0)`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Temp {
    pub label: String,
    pub celsius: f64,
}

/// Everything useful a single hwmon directory exposes.
#[derive(Debug, Clone, Default)]
pub struct HwmonReadout {
    pub temps: Vec<Temp>,
    /// Fans in RPM, first fan first.
    pub fans_rpm: Vec<f64>,
    /// Static power limits in watts, if exposed.
    pub power_limit_w: Option<f64>,
    /// Monotonic energy counters in microjoules, one per `energyN_input`.
    /// Delta two samples to get power draw.
    pub energy_uj: Vec<u64>,
}

/// Read a hwmon-style attribute as f64, returning None for missing
/// or empty/erroring files (some ACPI fan nodes error on read).
pub fn read_f64(p: &Path) -> Option<f64> {
    fs::read_to_string(p).ok()?.trim().parse::<f64>().ok()
}

pub fn read_u64(p: &Path) -> Option<u64> {
    fs::read_to_string(p).ok()?.trim().parse::<u64>().ok()
}

/// Read the friendly name of a hwmon device ("coretemp", "xe", ...).
pub fn name(dir: &Path) -> String {
    fs::read_to_string(dir.join("name"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

/// Fully read one hwmon directory.
pub fn readout(dir: &Path) -> HwmonReadout {
    let mut out = HwmonReadout::default();

    // Collect numbered files so we can iterate them in numeric order.
    // Filenames look like `temp10_input`, `fan1_input`, `energy1_label`.
    let mut entries: Vec<(u32, &'static str, String)> = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let file_name = e.file_name().to_string_lossy().into_owned();
            for kind in ["temp", "fan", "energy", "power"] {
                if let Some(rest) = file_name.strip_prefix(kind) {
                    // rest is like "10_input"; the number runs to '_'.
                    let num = rest.split('_').next().unwrap_or("");
                    if let Ok(n) = num.parse::<u32>() {
                        entries.push((n, kind, file_name));
                        break;
                    }
                }
            }
        }
    }
    entries.sort();

    for (n, kind, file_name) in &entries {
        match *kind {
            "temp" => {
                // Only *_input files; labels are read alongside.
                if !file_name.ends_with("_input") {
                    continue;
                }
                if let Some(c) = read_f64(&dir.join(format!("temp{n}_input"))) {
                    let celsius = c / 1000.0;
                    let label = fs::read_to_string(dir.join(format!("temp{n}_label")))
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|_| format!("temp{n}"));
                    out.temps.push(Temp { label, celsius });
                }
            }
            "fan" => {
                if file_name.ends_with("_input") {
                    if let Some(rpm) = read_f64(&dir.join(file_name)) {
                        out.fans_rpm.push(rpm);
                    }
                }
            }
            "energy" => {
                if file_name.ends_with("_input") {
                    if let Some(uj) = read_u64(&dir.join(file_name)) {
                        out.energy_uj.push(uj);
                    }
                }
            }
            "power" => {
                // Prefer power1_cap (xe) then power1_max (i915),
                // then power1_crit. Values are microvolts... rather:
                // micro-watts. 275000000 uW = 275 W.
                if let Some(v) = read_f64(&dir.join("power1_cap")) {
                    out.power_limit_w = Some(v / 1_000_000.0);
                } else if let Some(v) = read_f64(&dir.join("power1_max")) {
                    out.power_limit_w = Some(v / 1_000_000.0);
                } else if out.power_limit_w.is_none() {
                    if let Some(v) = read_f64(&dir.join("power1_crit")) {
                        out.power_limit_w = Some(v / 1_000_000.0);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// All hwmon directories under `/sys/class/hwmon`, in index order.
pub fn all() -> Vec<PathBuf> {
    let mut dirs: Vec<(u32, PathBuf)> = Vec::new();
    if let Ok(rd) = fs::read_dir("/sys/class/hwmon") {
        for e in rd.flatten() {
            let file_name = e.file_name().to_string_lossy().into_owned();
            if let Ok(n) = file_name.trim_start_matches("hwmon").parse::<u32>() {
                dirs.push((n, e.path()));
            }
        }
    }
    dirs.sort();
    dirs.into_iter().map(|(_, p)| p).collect()
}

/// Enumerate the hwmon directories that belong to one DRM card
/// (`/sys/class/drm/cardX/device/hwmon/hwmon*`).
pub fn for_card(device_dir: &Path) -> Vec<PathBuf> {
    let base = device_dir.join("hwmon");
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(&base) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}
