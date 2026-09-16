//! CPU page: per-core grid, clocks, temps, package power.

use super::{heading, plot, spark, temp_str, theme, tile, tile_bar};
use crate::collector::Snapshot;
use crate::history::Histories;
use egui::{RichText, Ui};

pub fn show(ui: &mut Ui, s: Option<&Snapshot>, hist: &Histories) {
    let Some(s) = s else { return };

    egui::ScrollArea::vertical().show(ui, |ui| {
        heading(ui, "CPU");
        ui.label(
            RichText::new(format!(
                "{} LOGICAL CORES{}",
                s.cpu.core_count,
                s.cpu
                    .base_mhz
                    .map(|b| format!(" · BASE CLOCK {:.0} MHz", b))
                    .unwrap_or_default()
            ))
            .size(11.0)
            .color(theme::DIM),
        );
        ui.add_space(8.0);

        // Headline tiles
        ui.horizontal(|ui| {
            tile_bar(
                ui,
                "Total utilization",
                format!("{:.1}%", s.cpu.utilization_pct),
                None,
                s.cpu.utilization_pct / 100.0,
                theme::UTIL,
            );
            tile(ui, "Package temp", temp_str(s.cpu.package_celsius), None);
            tile(
                ui,
                "Hottest core",
                temp_str(s.cpu.hottest_core_celsius),
                None,
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
        ui.add_space(8.0);

        plot(
            ui,
            "cpu-total-util",
            200.0,
            &[(
                "TOTAL UTILIZATION %".into(),
                hist.get("cpu.util")
                    .map(|r| r.tail(1200))
                    .unwrap_or_default(),
                theme::UTIL,
            )],
        );

        // Per-core sparkline grid
        ui.add_space(12.0);
        heading(ui, "LOGICAL PROCESSORS");
        ui.add_space(6.0);
        let cols = 6;
        egui::Grid::new("cores")
            .num_columns(cols)
            .spacing([10.0, 8.0])
            .show(ui, |ui| {
                for i in 0..s.cpu.core_count {
                    let util = s
                        .cpu
                        .per_core_utilization_pct
                        .get(i)
                        .copied()
                        .unwrap_or(0.0);
                    let freq = s.cpu.per_core_mhz.get(i).copied().unwrap_or(0.0);
                    let temp = s.cpu.per_core_celsius.get(i).copied();
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "{:>2} {:>3.0}% {:>4.0}MHz {}",
                                i,
                                util,
                                freq,
                                temp.map(|t| format!("{t:>3.0}°"))
                                    .unwrap_or_else(|| "  ──".into())
                            ))
                            .size(10.0)
                            .color(theme::DIM),
                        );
                        spark(
                            ui,
                            &format!("core-spark-{i}"),
                            hist.get(&format!("cpu.core.{i}")),
                            theme::UTIL,
                            150.0,
                        );
                    });
                    if (i + 1) % cols == 0 {
                        ui.end_row();
                    }
                }
            });

        ui.add_space(12.0);
        heading(ui, "CLOCKS · THERMALS · POWER");
        ui.add_space(6.0);
        let temp_series = (
            "PACKAGE °C".into(),
            hist.get("cpu.temp.pkg")
                .map(|r| r.tail(1200))
                .unwrap_or_default(),
            theme::TEMP,
        );
        let hot_series = (
            "HOTTEST CORE °C".into(),
            hist.get("cpu.temp.core")
                .map(|r| r.tail(1200))
                .unwrap_or_default(),
            theme::TEMP,
        );
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("FREQUENCY").size(10.0).color(theme::DIM));
                plot(
                    ui,
                    "cpu-freq",
                    180.0,
                    &[(
                        "AVERAGE CLOCK MHz".into(),
                        hist.get("cpu.freq")
                            .map(|r| r.tail(1200))
                            .unwrap_or_default(),
                        theme::FREQ,
                    )],
                );
            });
            ui.vertical(|ui| {
                ui.label(RichText::new("TEMPERATURE").size(10.0).color(theme::DIM));
                plot(ui, "cpu-temp", 180.0, &[temp_series, hot_series]);
            });
            ui.vertical(|ui| {
                ui.label(RichText::new("POWER").size(10.0).color(theme::DIM));
                plot(
                    ui,
                    "cpu-power",
                    180.0,
                    &[(
                        "PACKAGE POWER W".into(),
                        hist.get("cpu.power")
                            .map(|r| r.tail(1200))
                            .unwrap_or_default(),
                        theme::POWER,
                    )],
                );
            });
        });
    });
}
