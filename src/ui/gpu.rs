//! Per-GPU detail page — the full dossier for one adapter.

use super::{bar, heading, plot, spark, temp_str, theme, tile, tile_bar};
use crate::collector::gpu::GpuSample;
use crate::history::Histories;
use egui::{RichText, Ui};

pub fn show(ui: &mut Ui, g: &GpuSample, hist: &Histories, index: usize) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        heading(ui, &g.model.to_uppercase());
        ui.label(
            RichText::new(format!(
                "CARD{} · {} · {}{}",
                g.card,
                g.pci,
                g.driver,
                if g.integrated {
                    " · INTEGRATED"
                } else {
                    " · DISCRETE"
                }
            ))
            .size(11.0)
            .color(theme::DIM),
        );
        ui.add_space(10.0);

        // Headline tiles
        ui.horizontal(|ui| {
            tile_bar(
                ui,
                "Utilization",
                format!("{:.0}%", g.utilization_pct),
                None,
                g.utilization_pct / 100.0,
                theme::UTIL,
            );
            tile(
                ui,
                "Clock",
                g.cur_mhz
                    .map(|f| format!("{f:.0} MHz"))
                    .unwrap_or_else(|| "──".into()),
                g.max_mhz.map(|m| format!("MAX {m:.0} MHz")),
            );
            tile(ui, "Temperature", temp_str(g.temp_c), None);
            tile(
                ui,
                "Power draw",
                g.power_w
                    .map(|p| format!("{p:.1} W"))
                    .unwrap_or_else(|| "──".into()),
                g.power_limit_w.map(|c| format!("LIMIT {c:.0} W")),
            );
            tile(
                ui,
                "Fan",
                g.fan_rpm
                    .map(|f| format!("{f:.0} RPM"))
                    .unwrap_or_else(|| "──".into()),
                None,
            );
        });
        ui.add_space(8.0);

        // Utilization plot + engine breakdown
        plot(
            ui,
            &format!("gpu{index}-util-plot"),
            210.0,
            &[
                (
                    "TOTAL %".into(),
                    hist.get(&format!("gpu{}.util", g.card))
                        .map(|r| r.tail(1200))
                        .unwrap_or_default(),
                    theme::UTIL,
                ),
                (
                    "RENDER %".into(),
                    hist.get(&format!("gpu{}.engine.rcs", g.card))
                        .map(|r| r.tail(1200))
                        .unwrap_or_default(),
                    theme::VRAM,
                ),
                (
                    "VIDEO %".into(),
                    hist.get(&format!("gpu{}.engine.vcs", g.card))
                        .map(|r| r.tail(1200))
                        .unwrap_or_default(),
                    theme::FREQ,
                ),
                (
                    "COMPUTE %".into(),
                    hist.get(&format!("gpu{}.engine.ccs", g.card))
                        .map(|r| r.tail(1200))
                        .unwrap_or_default(),
                    theme::POWER,
                ),
            ],
        );

        if !g.engines.is_empty() {
            ui.add_space(10.0);
            heading(ui, "ENGINES");
            ui.add_space(6.0);
            egui::Grid::new(format!("gpu{index}-engines"))
                .num_columns(3)
                .spacing([10.0, 8.0])
                .show(ui, |ui| {
                    for e in &g.engines {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{} {:>3.0}%",
                                    e.label.to_uppercase(),
                                    e.utilization_pct
                                ))
                                .size(11.0)
                                .color(theme::TEXT),
                            );
                            bar(ui, e.utilization_pct / 100.0, theme::UTIL, 150.0);
                            spark(
                                ui,
                                &format!("gpu{}-eng-{}", g.card, e.key),
                                hist.get(&format!("gpu{}.engine.{}", g.card, e.key)),
                                theme::UTIL,
                                150.0,
                            );
                        });
                    }
                });
        }

        // Clocks / power / thermals / fan plots
        ui.add_space(12.0);
        heading(ui, "CLOCKS · POWER · THERMALS");
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("CLOCK MHz").size(10.0).color(theme::DIM));
                plot(
                    ui,
                    &format!("gpu{index}-freq-plot"),
                    170.0,
                    &[(
                        "MHz".into(),
                        hist.get(&format!("gpu{}.freq", g.card))
                            .map(|r| r.tail(1200))
                            .unwrap_or_default(),
                        theme::FREQ,
                    )],
                );
            });
            ui.vertical(|ui| {
                ui.label(RichText::new("POWER W").size(10.0).color(theme::DIM));
                plot(
                    ui,
                    &format!("gpu{index}-power-plot"),
                    170.0,
                    &[(
                        "WATTS".into(),
                        hist.get(&format!("gpu{}.power", g.card))
                            .map(|r| r.tail(1200))
                            .unwrap_or_default(),
                        theme::POWER,
                    )],
                );
            });
            ui.vertical(|ui| {
                ui.label(RichText::new("PACKAGE °C").size(10.0).color(theme::DIM));
                plot(
                    ui,
                    &format!("gpu{index}-temp-plot"),
                    170.0,
                    &[(
                        "°C".into(),
                        hist.get(&format!("gpu{}.temp", g.card))
                            .map(|r| r.tail(1200))
                            .unwrap_or_default(),
                        theme::TEMP,
                    )],
                );
            });
            ui.vertical(|ui| {
                ui.label(RichText::new("FAN RPM").size(10.0).color(theme::DIM));
                plot(
                    ui,
                    &format!("gpu{index}-fan-plot"),
                    170.0,
                    &[(
                        "RPM".into(),
                        hist.get(&format!("gpu{}.fan", g.card))
                            .map(|r| r.tail(1200))
                            .unwrap_or_default(),
                        theme::FAN,
                    )],
                );
            });
        });

        // All temperature sensors (xe exposes pkg, vram, mctrl, pcie and
        // up to 16 VRAM channel sensors).
        if g.temps.len() > 1 {
            ui.add_space(12.0);
            heading(ui, "THERMAL SENSOR MAP");
            ui.add_space(6.0);
            let mut sensors: Vec<_> = g.temps.iter().collect();
            sensors.sort_by(|a, b| b.celsius.partial_cmp(&a.celsius).unwrap());
            egui::Grid::new(format!("gpu{index}-temps"))
                .num_columns(4)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    for t in sensors {
                        let hot = t.celsius >= 80.0;
                        ui.label(
                            RichText::new(format!("{}: {:>3.0} °C", t.label, t.celsius))
                                .size(11.0)
                                .color(if hot { theme::TEMP } else { theme::TEXT }),
                        );
                    }
                });
        }
    });
}
