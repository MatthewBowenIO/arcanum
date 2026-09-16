//! UI pages and shared widgets, in the amber-CRT house style.

pub mod cpu;
pub mod gpu;
pub mod memory;
pub mod summary;

use crate::collector::gpu::GpuSample;
use crate::history::{Histories, Ring, CAPACITY};
use egui::{Color32, RichText, Ui};
use egui_plot::{Legend, Line, Plot, PlotPoints};

/// Which page is visible.
#[derive(Debug, Clone, PartialEq)]
pub enum Tab {
    Summary,
    Cpu,
    Memory,
    Gpu(usize),
}

/// The amber-CRT palette. One place to tweak the whole look.
pub mod theme {
    use egui::Color32;

    /// Main background (near black).
    pub const BG: Color32 = Color32::from_rgb(9, 11, 10);
    /// Group / panel background.
    pub const PANEL: Color32 = Color32::from_rgb(14, 17, 15);
    /// Plot background (slightly darker than panels).
    pub const PLOT_BG: Color32 = Color32::from_rgb(6, 8, 7);
    /// Border / grid stroke.
    pub const GRID: Color32 = Color32::from_rgb(48, 42, 26);

    /// Primary accent — amber phosphor.
    pub const AMBER: Color32 = Color32::from_rgb(255, 176, 0);
    pub const AMBER_DIM: Color32 = Color32::from_rgb(120, 82, 10);
    /// Body text — pale amber.
    pub const TEXT: Color32 = Color32::from_rgb(214, 199, 160);
    /// Captions and metadata.
    pub const DIM: Color32 = Color32::from_rgb(133, 118, 84);
    /// Selected-row highlight.
    pub const SELECT: Color32 = Color32::from_rgb(52, 36, 6);

    // Metric colors — classic terminal set.
    pub const UTIL: Color32 = AMBER; // utilization
    pub const FREQ: Color32 = Color32::from_rgb(86, 200, 255); // cyan
    pub const TEMP: Color32 = Color32::from_rgb(255, 95, 82); // red
    pub const POWER: Color32 = Color32::from_rgb(255, 126, 219); // magenta
    pub const FAN: Color32 = Color32::from_rgb(82, 224, 82); // green
    pub const VRAM: Color32 = Color32::from_rgb(120, 170, 240); // blue
    pub const MEM: Color32 = Color32::from_rgb(82, 224, 82); // green
    pub const RED: Color32 = Color32::from_rgb(255, 95, 82);
}

/// Section heading in the house style: `─── TITLE ───`.
pub fn heading(ui: &mut Ui, title: &str) {
    ui.label(
        RichText::new(format!("─── {title} ───"))
            .color(theme::AMBER)
            .size(14.0)
            .strong(),
    );
}

/// Big metric readout: uppercase caption, bright value, optional sub-line.
pub fn tile(ui: &mut Ui, caption: &str, value: String, sub: Option<String>) {
    ui.vertical(|ui| {
        ui.label(
            RichText::new(caption.to_uppercase())
                .size(10.0)
                .color(theme::DIM),
        );
        ui.label(RichText::new(value).size(20.0).strong().color(theme::AMBER));
        if let Some(sub) = sub {
            ui.label(
                RichText::new(sub.to_uppercase())
                    .size(10.0)
                    .color(theme::DIM),
            );
        }
    });
}

/// A horizontal utilization bar, 0..1. Width must be explicit:
/// egui's ProgressBar fills all remaining width by default, which
/// explodes inside horizontal rows and grid cells.
pub fn bar(ui: &mut Ui, frac: f64, color: Color32, width: f32) {
    let frac = frac.clamp(0.0, 1.0);
    ui.add(
        egui::ProgressBar::new(frac as f32)
            .fill(color)
            .desired_height(12.0)
            .desired_width(width),
    );
}

/// [`tile`] with a bar underneath, sized to the tile's own width.
/// For headline readouts ("TOTAL UTILIZATION  3.4%  ▮▮▮▮░░░").
#[allow(clippy::too_many_arguments)]
pub fn tile_bar(
    ui: &mut Ui,
    caption: &str,
    value: String,
    sub: Option<String>,
    frac: f64,
    color: Color32,
) {
    ui.vertical(|ui| {
        ui.label(
            RichText::new(caption.to_uppercase())
                .size(10.0)
                .color(theme::DIM),
        );
        ui.label(RichText::new(value).size(20.0).strong().color(theme::AMBER));
        if let Some(sub) = sub {
            ui.label(
                RichText::new(sub.to_uppercase())
                    .size(10.0)
                    .color(theme::DIM),
            );
        }
        // Width of the widest label so far, so the bar matches the tile.
        let w = ui.min_rect().width();
        bar(ui, frac, color, w);
    });
}

/// A small line chart without axes or grid. Like the bar above, the
/// width is explicit: egui_plot fills remaining width by default.
pub fn spark(ui: &mut Ui, id: &str, ring: Option<&Ring>, color: Color32, width: f32) {
    let points = ring.map(|r| r.tail(CAPACITY)).unwrap_or_default();
    Plot::new(id)
        .width(width)
        .height(40.0)
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .show_grid(false)
        .show_axes([false, false])
        .show_background(false)
        .show(ui, |pu| {
            pu.line(
                Line::new("", PlotPoints::from(points))
                    .color(color)
                    .width(1.5),
            );
        });
}

/// A full plot with legend for one or more named series.
pub fn plot(ui: &mut Ui, id: &str, height: f32, series: &[(String, Vec<[f64; 2]>, Color32)]) {
    Plot::new(id)
        .height(height)
        .allow_zoom(true)
        .allow_drag(true)
        .allow_scroll(false)
        .show_grid(true)
        .grid_color(theme::GRID)
        .show_background(true)
        .legend(
            Legend::default()
                .text_style(egui::TextStyle::Small)
                .background_alpha(0.0),
        )
        .show(ui, |pu| {
            for (name, points, color) in series {
                pu.line(
                    Line::new(name.clone(), PlotPoints::from(points.clone()))
                        .color(*color)
                        .width(1.5),
                );
            }
        });
}

/// One GPU summary card for the Summary page.
pub fn gpu_card(ui: &mut Ui, hist: &Histories, g: &GpuSample) {
    egui::Frame::new()
        .fill(theme::PANEL)
        .stroke(egui::Stroke::new(1.0, theme::GRID))
        .inner_margin(egui::Margin::same(10))
        .outer_margin(2)
        .show(ui, |ui| {
            ui.set_min_width(300.0);
            ui.vertical(|ui| {
                // Title line: model + driver tag
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(g.model.to_uppercase())
                            .strong()
                            .color(theme::AMBER)
                            .size(13.0),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        let tag = match (g.driver, g.integrated) {
                            ("i915", true) => "I915·IGPU",
                            ("i915", false) => "I915·DISCRETE",
                            (_, true) => "XE·IGPU",
                            _ => "XE·DISCRETE",
                        };
                        ui.label(RichText::new(tag).size(10.0).color(theme::DIM));
                    });
                });
                ui.add_space(4.0);

                // Utilization + clock
                ui.horizontal(|ui| {
                    tile(ui, "Util", format!("{:>3.0}%", g.utilization_pct), None);
                    ui.separator();
                    tile(
                        ui,
                        "Clock",
                        g.cur_mhz
                            .map(|f| format!("{:>4.0}", f))
                            .unwrap_or_else(|| "──".into()),
                        Some("MHz".into()),
                    );
                });
                // Card content width: min 300 (set above), or wider if
                // the model name needs it. Explicit so the bar and spark
                // stay inside the card instead of filling the whole row.
                let card_width = ui.min_rect().width().max(300.0) - 8.0;
                bar(ui, g.utilization_pct / 100.0, theme::UTIL, card_width);
                spark(
                    ui,
                    &format!("gpu{}-util", g.card),
                    hist.get(&format!("gpu{}.util", g.card)),
                    theme::UTIL,
                    card_width,
                );

                egui::Grid::new(format!("gpu{}-stats", g.card))
                    .num_columns(2)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("TEMP").size(10.0).color(theme::DIM));
                        ui.label(temp_str(g.temp_c));
                        ui.end_row();
                        ui.label(RichText::new("POWER").size(10.0).color(theme::DIM));
                        ui.label(match (g.power_w, g.power_limit_w) {
                            (Some(p), Some(c)) => format!("{p:>5.1} / {c:<3.0} W"),
                            (Some(p), None) => format!("{p:>5.1} W"),
                            _ => "──".into(),
                        });
                        ui.end_row();
                        ui.label(RichText::new("FAN").size(10.0).color(theme::DIM));
                        ui.label(
                            g.fan_rpm
                                .map(|f| format!("{f:>4.0} RPM"))
                                .unwrap_or_else(|| "──".into()),
                        );
                        ui.end_row();
                        ui.label(RichText::new("MEM").size(10.0).color(theme::DIM));
                        ui.label(match (g.vram_used_mb, g.shared_used_mb) {
                            (Some(v), _) => format!("{v:>5.0} MiB VRAM"),
                            (None, Some(s)) => format!("{s:>5.0} MiB SHARED"),
                            _ => "──".into(),
                        });
                        ui.end_row();
                    });
            });
        });
}

pub fn temp_str(c: Option<f64>) -> String {
    c.map(|c| format!("{c:>4.0} °C"))
        .unwrap_or_else(|| "──".into())
}

/// Shown when no Intel GPU was found.
pub fn no_gpu_hint(ui: &mut Ui) {
    ui.centered_and_justified(|ui| {
        ui.label(
            RichText::new(
                "NO INTEL GPU FOUND (I915 / XE)\nNVIDIA / AMD ARE OUT OF SCOPE BY DESIGN",
            )
            .color(theme::AMBER),
        );
    });
}

pub fn snapshot_hint(ui: &mut Ui) {
    ui.centered_and_justified(|ui| {
        ui.label(RichText::new("AWAITING FIRST SAMPLE…").color(theme::AMBER));
    });
}
