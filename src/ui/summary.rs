//! Summary page: everything at a glance, arranged as a trading floor.

use super::{gpu_card, heading, plot, temp_str, theme, tile, tile_bar};
use crate::collector::Snapshot;
use crate::history::Histories;
use egui::{RichText, Ui};

pub fn show(ui: &mut Ui, s: Option<&Snapshot>, hist: &Histories) {
    let Some(s) = s else { return };

    egui::ScrollArea::vertical().show(ui, |ui| {
        // ── Adapter row ────────────────────────────────────────────
        heading(ui, "ADAPTERS");
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            for g in &s.gpus {
                gpu_card(ui, hist, g);
            }
            if s.gpus.is_empty() {
                ui.label(RichText::new("NO INTEL ADAPTERS DETECTED.").color(theme::RED));
            }
        });

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(14.0);

        // ── CPU ─────────────────────────────────────────────────────
        heading(ui, "CPU");
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            tile_bar(
                ui,
                "Utilization",
                format!("{:.1}%", s.cpu.utilization_pct),
                None,
                s.cpu.utilization_pct / 100.0,
                theme::UTIL,
            );
            tile(ui, "Package", temp_str(s.cpu.package_celsius), {
                s.cpu
                    .hottest_core_celsius
                    .map(|h| format!("HOTTEST CORE {h:.0}°C"))
            });
            let avg = avg_clock(&s.cpu.per_core_mhz);
            tile(
                ui,
                "Clock",
                avg.map(|a| format!("{a:.0} MHz"))
                    .unwrap_or_else(|| "──".into()),
                s.cpu.base_mhz.map(|b| format!("BASE {b:.0} MHz")),
            );
            tile(
                ui,
                "Package power",
                s.cpu
                    .package_power_w
                    .map(|p| format!("{p:.1} W"))
                    .unwrap_or_else(|| "NEEDS ROOT".into()),
                None,
            );
        });
        ui.add_space(6.0);
        plot(
            ui,
            "summary-cpu-util",
            180.0,
            &[(
                "CPU UTILIZATION %".into(),
                hist.get("cpu.util")
                    .map(|r| r.tail(600))
                    .unwrap_or_default(),
                theme::UTIL,
            )],
        );

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(14.0);

        // ── Memory ──────────────────────────────────────────────────
        heading(ui, "MEMORY");
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            tile_bar(
                ui,
                "In use",
                format!("{:.1} GiB", s.memory.used_mb / 1024.0),
                Some(format!("OF {:.1} GiB", s.memory.total_mb / 1024.0)),
                s.memory.used_fraction,
                theme::MEM,
            );
            tile(
                ui,
                "Available",
                format!("{:.1} GiB", s.memory.available_mb / 1024.0),
                Some(format!("CACHED {:.1} GiB", s.memory.cached_mb / 1024.0)),
            );
            if s.memory.swap_total_mb > 0.0 {
                tile(
                    ui,
                    "Swap",
                    format!(
                        "{:.1} / {:.1} GiB",
                        s.memory.swap_used_mb / 1024.0,
                        s.memory.swap_total_mb / 1024.0
                    ),
                    None,
                );
            }
        });
        ui.add_space(6.0);
        plot(
            ui,
            "summary-mem",
            180.0,
            &[
                (
                    "USED MiB".into(),
                    hist.get("mem.used")
                        .map(|r| r.tail(600))
                        .unwrap_or_default(),
                    theme::MEM,
                ),
                (
                    "AVAILABLE MiB".into(),
                    hist.get("mem.available")
                        .map(|r| r.tail(600))
                        .unwrap_or_default(),
                    theme::VRAM,
                ),
            ],
        );

        if !s.system_fans.is_empty() {
            ui.add_space(14.0);
            ui.separator();
            ui.add_space(6.0);
            heading(ui, "SYSTEM FANS");
            for f in &s.system_fans {
                ui.label(format!("{}: {:>4.0} RPM", f.label.to_uppercase(), f.rpm));
            }
        }
    });
}

fn avg_clock(per_core_mhz: &[f64]) -> Option<f64> {
    let live: Vec<&f64> = per_core_mhz.iter().filter(|m| **m > 0.0).collect();
    if live.is_empty() {
        None
    } else {
        Some(live.iter().map(|m| **m).sum::<f64>() / live.len() as f64)
    }
}
