//! The eframe application: CRT-terminal theme, panel layout, history
//! recording, and routing to UI pages.

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use crate::collector::Snapshot;
use crate::history::Histories;
use crate::ui;

pub struct ArcanumApp {
    rx: Receiver<Snapshot>,
    snapshot: Option<Snapshot>,
    hist: Histories,
    tab: ui::Tab,
    started: Instant,
}

impl ArcanumApp {
    pub fn new(rx: Receiver<Snapshot>) -> Self {
        Self {
            rx,
            snapshot: None,
            hist: Histories::default(),
            tab: ui::Tab::Summary,
            started: Instant::now(),
        }
    }
}

/// Install the retro-terminal look: monospace everything, amber-on-black.
pub fn install_theme(ctx: &egui::Context) {
    // ── Fonts: use the built-in monospace (Hack) for everything ──
    let mut fonts = egui::FontDefinitions::default();
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        // "Hack" ships as the default monospace face in egui.
        family.insert(0, "Hack".to_owned());
    }
    ctx.set_fonts(fonts);

    // ── Palette: amber CRT ──
    ctx.set_theme(egui::Theme::Dark);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    let v = &mut style.visuals;
    v.dark_mode = true;
    v.panel_fill = ui::theme::BG;
    v.extreme_bg_color = ui::theme::PLOT_BG;
    v.faint_bg_color = ui::theme::PANEL;
    v.window_fill = ui::theme::PANEL;
    v.override_text_color = Some(ui::theme::TEXT);
    v.hyperlink_color = ui::theme::AMBER;
    v.warn_fg_color = ui::theme::RED;
    v.error_fg_color = ui::theme::RED;
    v.selection.bg_fill = ui::theme::AMBER_DIM;
    v.selection.stroke = egui::Stroke::new(1.0, ui::theme::BG);

    // Widgets: flat, stroked — like a text UI.
    let flat = |w: &mut egui::style::WidgetVisuals| {
        w.bg_fill = egui::Color32::TRANSPARENT;
        w.bg_stroke = egui::Stroke::new(1.0, ui::theme::GRID);
        w.fg_stroke = egui::Stroke::new(1.0, ui::theme::TEXT);
    };
    flat(&mut v.widgets.noninteractive);
    flat(&mut v.widgets.inactive);
    flat(&mut v.widgets.hovered);
    flat(&mut v.widgets.active);
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, ui::theme::AMBER);
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ui::theme::AMBER_DIM);
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, ui::theme::AMBER);
    v.widgets.active.bg_fill = ui::theme::SELECT;
    v.widgets.open.bg_fill = ui::theme::PANEL;
    v.widgets.open.fg_stroke = egui::Stroke::new(1.0, ui::theme::AMBER);

    // Panels drawn by us get thin grid-colored borders.
    v.window_stroke = egui::Stroke::new(1.0, ui::theme::GRID);
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, ui::theme::GRID);

    ctx.set_style_of(egui::Theme::Dark, style);
}

impl eframe::App for ArcanumApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Consume all pending snapshots; keep the newest.
        while let Ok(s) = self.rx.try_recv() {
            self.snapshot = Some(s);
        }
        if let Some(s) = &self.snapshot {
            self.hist.record(s.uptime_ms);
            record_histories(&mut self.hist, s);
        }

        // ── Ticker header ────────────────────────────────────────────
        egui::Panel::top("header").show(ui, |ui| {
            ui.vertical(|ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    // Blinking block cursor, 1 Hz.
                    let cursor = if (ui.ctx().time() * 2.0) as i64 % 2 == 0 {
                        "▮"
                    } else {
                        " "
                    };
                    ui.label(
                        egui::RichText::new("ARCANUM ")
                            .strong()
                            .color(ui::theme::AMBER)
                            .size(19.0),
                    );
                    ui.label(
                        egui::RichText::new(cursor)
                            .color(ui::theme::AMBER)
                            .size(19.0),
                    );
                    ui.separator();
                    ui.label(egui::RichText::new("INTEL TELEMETRY").size(12.0).weak());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(s) = &self.snapshot {
                            ui.label(
                                egui::RichText::new(self.status_line(s))
                                    .size(12.0)
                                    .color(ui::theme::TEXT),
                            );
                        } else {
                            ui.label(
                                egui::RichText::new("CONNECTING…")
                                    .size(12.0)
                                    .color(ui::theme::AMBER),
                            );
                        }
                    });
                });
                ui.add_space(2.0);
            });
        });

        // ── Warnings ticker ─────────────────────────────────────────
        if let Some(s) = &self.snapshot {
            if !s.warnings.is_empty() {
                egui::Panel::bottom("warnings").show(ui, |ui| {
                    ui.add_space(4.0);
                    for w in &s.warnings {
                        ui.label(
                            egui::RichText::new(format!("!! {w}"))
                                .color(ui::theme::RED)
                                .size(12.0),
                        );
                    }
                    ui.add_space(4.0);
                });
            }
        }

        // ── Nav ─────────────────────────────────────────────────────
        egui::Panel::left("nav").show(ui, |ui| {
            ui.set_min_width(300.0);
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new("═══ CONSOLE ═══")
                        .color(ui::theme::AMBER_DIM)
                        .size(11.0)
                        .strong(),
                );
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.selectable_value(&mut self.tab, ui::Tab::Summary, "▸ SUMMARY");
                    ui.selectable_value(&mut self.tab, ui::Tab::Cpu, "▸ CPU");
                    ui.selectable_value(&mut self.tab, ui::Tab::Memory, "▸ MEMORY");
                });
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("═══ ADAPTERS ═══")
                        .color(ui::theme::AMBER_DIM)
                        .size(11.0)
                        .strong(),
                );
                ui.add_space(8.0);

                let gpu_count = self.snapshot.as_ref().map(|s| s.gpus.len()).unwrap_or(0);
                for i in 0..gpu_count {
                    let g = &self.snapshot.as_ref().unwrap().gpus[i];
                    ui.vertical(|ui| {
                        ui.selectable_value(
                            &mut self.tab,
                            ui::Tab::Gpu(i),
                            egui::RichText::new(format!("▸ GPU {i}  {}", g.model.to_uppercase()))
                                .size(13.0),
                        );
                        ui.label(
                            egui::RichText::new(format!(
                                "   {} · {}{}",
                                g.driver,
                                if g.integrated {
                                    "INTEGRATED"
                                } else {
                                    "DISCRETE"
                                },
                                if let Some(t) = g.temp_c {
                                    format!(" · {:>3.0}°C", t)
                                } else {
                                    String::new()
                                }
                            ))
                            .size(10.0)
                            .color(ui::theme::DIM),
                        );
                        ui.add_space(6.0);
                    });
                }
                if gpu_count == 0 {
                    ui.label(
                        egui::RichText::new("NO INTEL ADAPTERS DETECTED")
                            .size(11.0)
                            .weak(),
                    );
                }
            });
        });

        // ── Content ─────────────────────────────────────────────────
        // Inner margin keeps plots and legends from running flush
        // against the window edge.
        let frame = egui::Frame::new()
            .inner_margin(egui::Margin {
                left: 10,
                right: 14,
                top: 4,
                bottom: 10,
            })
            .fill(ui::theme::BG);
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            if self.snapshot.is_none() {
                ui::snapshot_hint(ui);
                return;
            }
            let s = self.snapshot.as_ref().unwrap();
            let gpu_count = s.gpus.len();
            match self.tab {
                ui::Tab::Summary => ui::summary::show(ui, Some(s), &self.hist),
                ui::Tab::Cpu => ui::cpu::show(ui, Some(s), &self.hist),
                ui::Tab::Memory => ui::memory::show(ui, Some(s), &self.hist),
                ui::Tab::Gpu(i) if i < gpu_count => ui::gpu::show(ui, &s.gpus[i], &self.hist, i),
                _ => ui::no_gpu_hint(ui),
            }
        });

        // Poll at the sampler's cadence; egui redraws on input anyway.
        ui.ctx().request_repaint_after(Duration::from_millis(250));
    }
}

impl ArcanumApp {
    /// Terminal-style status: T+ runtime, core/GPU counts.
    fn status_line(&self, s: &Snapshot) -> String {
        let up = self.started.elapsed().as_secs();
        format!(
            "T+{:02}:{:02}  │  {} CORES  │  {} GPU",
            up / 60,
            up % 60,
            s.cpu.core_count,
            s.gpus.len()
        )
    }
}

fn record_histories(hist: &mut Histories, s: &Snapshot) {
    hist.push("cpu.util", s.cpu.utilization_pct);
    for (i, u) in s.cpu.per_core_utilization_pct.iter().enumerate() {
        hist.push(&format!("cpu.core.{i}"), *u);
    }
    let live_clocks: Vec<f64> = s
        .cpu
        .per_core_mhz
        .iter()
        .copied()
        .filter(|m| *m > 0.0)
        .collect();
    let avg_freq = if live_clocks.is_empty() {
        0.0
    } else {
        live_clocks.iter().sum::<f64>() / live_clocks.len() as f64
    };
    hist.push("cpu.freq", avg_freq);
    if let Some(t) = s.cpu.package_celsius {
        hist.push("cpu.temp.pkg", t);
    }
    if let Some(t) = s.cpu.hottest_core_celsius {
        hist.push("cpu.temp.core", t);
    }
    if let Some(p) = s.cpu.package_power_w {
        hist.push("cpu.power", p);
    }

    hist.push("mem.used", s.memory.used_mb);
    hist.push("mem.available", s.memory.available_mb);
    hist.push("mem.cached", s.memory.cached_mb);
    hist.push("mem.swap", s.memory.swap_used_mb);

    for g in &s.gpus {
        let id = g.card;
        hist.push(&format!("gpu{id}.util"), g.utilization_pct);
        for e in &g.engines {
            hist.push(&format!("gpu{id}.engine.{}", e.key), e.utilization_pct);
        }
        if let Some(f) = g.cur_mhz {
            hist.push(&format!("gpu{id}.freq"), f);
        }
        if let Some(t) = g.temp_c {
            hist.push(&format!("gpu{id}.temp"), t);
        }
        if let Some(p) = g.power_w {
            hist.push(&format!("gpu{id}.power"), p);
        }
        if let Some(f) = g.fan_rpm {
            hist.push(&format!("gpu{id}.fan"), f);
        }
        if let Some(v) = g.vram_used_mb {
            hist.push(&format!("gpu{id}.vram"), v);
        }
    }
}
