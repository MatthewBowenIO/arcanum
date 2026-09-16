//! Memory page.

use super::{bar, heading, plot, theme, tile, tile_bar};
use crate::collector::Snapshot;
use crate::history::Histories;
use egui::{RichText, Ui};

pub fn show(ui: &mut Ui, s: Option<&Snapshot>, hist: &Histories) {
    let Some(s) = s else { return };
    let m = &s.memory;

    ui.add_space(8.0);
    heading(ui, "MEMORY");
    ui.add_space(10.0);

    ui.horizontal(|ui| {
        tile_bar(
            ui,
            "In use",
            format!("{:.2} GiB", m.used_mb / 1024.0),
            Some(format!("{:.0}%", m.used_fraction * 100.0)),
            m.used_fraction,
            theme::MEM,
        );
        tile(
            ui,
            "Available",
            format!("{:.2} GiB", m.available_mb / 1024.0),
            None,
        );
        tile(
            ui,
            "Cached",
            format!("{:.2} GiB", m.cached_mb / 1024.0),
            None,
        );
        tile(
            ui,
            "Buffers",
            format!("{:.2} GiB", m.buffers_mb / 1024.0),
            None,
        );
    });
    ui.add_space(10.0);

    plot(
        ui,
        "mem-plot",
        260.0,
        &[
            (
                "USED MiB".into(),
                hist.get("mem.used")
                    .map(|r| r.tail(1200))
                    .unwrap_or_default(),
                theme::MEM,
            ),
            (
                "AVAILABLE MiB".into(),
                hist.get("mem.available")
                    .map(|r| r.tail(1200))
                    .unwrap_or_default(),
                theme::VRAM,
            ),
            (
                "CACHED MiB".into(),
                hist.get("mem.cached")
                    .map(|r| r.tail(1200))
                    .unwrap_or_default(),
                theme::FREQ,
            ),
        ],
    );

    if m.swap_total_mb > 0.0 {
        ui.add_space(12.0);
        heading(ui, "SWAP");
        ui.add_space(6.0);
        tile(
            ui,
            "Swap in use",
            format!(
                "{:.2} / {:.2} GiB",
                m.swap_used_mb / 1024.0,
                m.swap_total_mb / 1024.0
            ),
            Some(format!("{:.0}%", m.swap_used_fraction * 100.0)),
        );
        bar(ui, m.swap_used_fraction, theme::TEMP, 200.0);
        plot(
            ui,
            "swap-plot",
            150.0,
            &[(
                "SWAP USED MiB".into(),
                hist.get("mem.swap")
                    .map(|r| r.tail(1200))
                    .unwrap_or_default(),
                theme::TEMP,
            )],
        );
    } else {
        ui.add_space(12.0);
        ui.label(RichText::new("NO SWAP CONFIGURED.").color(theme::DIM));
    }
}
