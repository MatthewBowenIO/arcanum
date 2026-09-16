//! arcanum — a live hardware dashboard for Intel graphics.
//!
//! GUI is the default. Two headless modes exist for scripting and CI:
//!   arcanum --once          pretty-print one full sample
//!   arcanum --once --json   machine-readable JSON

mod app;
mod collector;
mod history;
mod ui;

use std::time::Duration;

const DEFAULT_INTERVAL_SECS: f64 = 1.0;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let once = args.iter().any(|a| a == "--once");
    let json = args.iter().any(|a| a == "--json");
    let interval = args
        .iter()
        .position(|a| a == "--interval")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(DEFAULT_INTERVAL_SECS)
        .clamp(0.25, 60.0);

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("arcanum {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }

    if once {
        if let Err(e) = run_once(json, interval) {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        return;
    }

    let (rx, _sampler) = collector::spawn_sampler(Duration::from_secs_f64(interval));

    let icon =
        eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1320.0, 900.0])
            .with_icon(std::sync::Arc::new(icon)),
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        "Arcanum",
        options,
        Box::new(move |cc| {
            app::install_theme(&cc.egui_ctx.clone());
            Ok(Box::new(app::ArcanumApp::new(rx)))
        }),
    ) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// Take one real measurement (prime + one delta window) and print it.
/// Needs to run ~2 intervals to produce meaningful rates.
fn run_once(json: bool, interval: f64) -> Result<(), String> {
    use collector::cpu::{CpuState, RaplState};
    use collector::gpu::GpuState;
    use collector::memory;

    let mut cpu_state = CpuState::new();
    let mut gpu_state = GpuState::new();
    let mut rapl = RaplState::new();

    // Prime counters.
    let _ = gpu_state.sample();
    let _ = cpu_state.sample();
    let _ = rapl.sample();
    std::thread::sleep(Duration::from_secs_f64(interval * 1.1));

    let mut cpu = cpu_state.sample();
    let mut gpus = gpu_state.sample();
    let mut warnings = gpu_state.take_warnings();
    let (pkg_power, denied) = rapl.sample();
    if denied {
        warnings.push(
            "CPU package power unavailable: RAPL energy counters require root. \
             See scripts/setup-permissions.sh"
                .into(),
        );
    }
    cpu.package_power_w = pkg_power;
    gpus.sort_by_key(|g| g.card);

    if json {
        let snapshot = collector::Snapshot {
            uptime_ms: 0,
            cpu,
            memory: memory::read(),
            gpus,
            system_fans: gpu_state.system_fans(),
            warnings,
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&snapshot).map_err(|e| e.to_string())?
        );
    } else {
        print_text(&cpu, &memory::read(), &gpus);
        if !warnings.is_empty() {
            eprintln!();
            for w in warnings {
                eprintln!("⚠ {w}");
            }
        }
    }
    Ok(())
}

fn print_text(
    cpu: &collector::cpu::CpuSample,
    mem: &collector::memory::MemorySample,
    gpus: &[collector::gpu::GpuSample],
) {
    println!("══════════ CPU ══════════");
    println!("cores: {}", cpu.core_count);
    println!("utilization: {:.1}%", cpu.utilization_pct);
    let live: Vec<f64> = cpu
        .per_core_mhz
        .iter()
        .copied()
        .filter(|m| *m > 0.0)
        .collect();
    println!(
        "clock: avg {:.0} MHz{}",
        live.iter().sum::<f64>() / live.len().max(1) as f64,
        cpu.base_mhz
            .map(|b| format!(" (base {b:.0})"))
            .unwrap_or_default()
    );
    println!(
        "temperature: package {} · hottest core {}",
        cpu.package_celsius
            .map(|t| format!("{t:.0} °C"))
            .unwrap_or_else(|| "–".into()),
        cpu.hottest_core_celsius
            .map(|t| format!("{t:.0} °C"))
            .unwrap_or_else(|| "–".into())
    );
    println!(
        "package power: {}",
        cpu.package_power_w
            .map(|p| format!("{p:.1} W"))
            .unwrap_or_else(|| "needs root".into())
    );

    println!("══════════ Memory ══════════");
    println!(
        "used {:.1} / total {:.1} GiB · available {:.1} GiB · cached {:.1} GiB · swap {:.1}/{:.1} GiB",
        mem.used_mb / 1024.0,
        mem.total_mb / 1024.0,
        mem.available_mb / 1024.0,
        mem.cached_mb / 1024.0,
        mem.swap_used_mb / 1024.0,
        mem.swap_total_mb / 1024.0
    );

    for g in gpus {
        println!("══════════ GPU card{} ══════════", g.card);
        println!("model: {} ({})", g.model, g.driver);
        println!("utilization: {:.1}%", g.utilization_pct);
        for e in &g.engines {
            println!(
                "  engine {} ({}): {:.1}%",
                e.label, e.key, e.utilization_pct
            );
        }
        println!(
            "clock: {}{}",
            g.cur_mhz
                .map(|f| format!("{f:.0} MHz"))
                .unwrap_or_else(|| "–".into()),
            g.max_mhz
                .map(|m| format!(" / {m:.0} MHz max"))
                .unwrap_or_default()
        );
        println!(
            "temperature: {}",
            g.temp_c
                .map(|t| format!("{t:.0} °C"))
                .unwrap_or_else(|| "–".into())
        );
        for t in &g.temps {
            println!("  {}: {:.0} °C", t.label, t.celsius);
        }
        println!(
            "power: {}{}",
            g.power_w
                .map(|p| format!("{p:.1} W"))
                .unwrap_or_else(|| "–".into()),
            g.power_limit_w
                .map(|c| format!(" (limit {c:.0} W)"))
                .unwrap_or_default()
        );
        println!(
            "fan: {}",
            g.fan_rpm
                .map(|f| format!("{f:.0} RPM"))
                .unwrap_or_else(|| "–".into())
        );
        println!(
            "memory: {}{}",
            g.vram_used_mb
                .map(|v| format!("{v:.0} MiB VRAM in use"))
                .unwrap_or_else(|| "–".into()),
            g.shared_used_mb
                .map(|v| format!(" · {v:.0} MiB shared"))
                .unwrap_or_default()
        );
    }
}

fn print_help() {
    println!(
        "arcanum {} — live hardware dashboard for Intel graphics (i915 / xe)

USAGE:
    arcanum                launch the GUI dashboard
    arcanum --once         print one full sample to stdout (text)
    arcanum --once --json  print one full sample as JSON
    arcanum --interval N   poll every N seconds (default 1)
    arcanum --version      print version

GPU support is intentionally Intel-only (i915 and xe kernel drivers).
See scripts/setup-permissions.sh to enable CPU package power (RAPL).
",
        env!("CARGO_PKG_VERSION")
    );
}
