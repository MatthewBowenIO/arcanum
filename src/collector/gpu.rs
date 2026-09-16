//! Intel GPU collection (i915 + xe drivers only — by design).
//!
//! Data sources, all plain kernel interfaces:
//! - `/sys/class/drm/card*/` — card enumeration, driver, clocks
//!   - i915: `gt_cur_freq_mhz` / `gt_max_freq_mhz` / ...
//!   - xe:   `device/tile*/gt*/freq0/{cur,max,min}_freq`
//! - `device/hwmon/hwmon*` — GPU temps (pkg, vram, vram channels),
//!   fan RPM, power limits, and energy counters (→ power draw)
//! - DRM `fdinfo` (`/proc/*/fdinfo/*`) — per-client per-engine
//!   utilization and memory, aggregated per card. This is the same
//!   interface nvtop and intel_gpu_top sit on, and it works
//!   unprivileged.
//!
//! NVIDIA/AMD are intentionally out of scope.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;

use super::hwmon;
use super::NamedFan;

/// Friendly names for DRM engine identifiers.
fn engine_label(key: &str) -> &str {
    match key {
        "rcs" | "render" => "Render",
        "ccs" | "compute" => "Compute",
        "bcs" | "copy" => "Copy",
        "vcs" | "video" => "Video",
        "vecs" | "video-enhance" => "Video Enhance",
        _ => "Other",
    }
}

// ---------------------------------------------------------------------
// Static card description

#[derive(Debug, Clone)]
struct GpuCard {
    /// DRM card number, e.g. 3 for card3.
    index: u32,
    pci: String,
    driver: &'static str, // "i915" | "xe"
    model: String,
    integrated: bool,
    sysfs: PathBuf, // /sys/class/drm/cardN
    hwmon_dirs: Vec<PathBuf>,
    /// renderD minors owned by this card; used to attribute fdinfo
    /// entries that lack a `drm-pdev` key.
    render_minors: Vec<u32>,
}

// ---------------------------------------------------------------------
// Per-poll sample types

#[derive(Debug, Clone, Serialize)]
pub struct EngineStat {
    /// Raw DRM engine key ("rcs", "vcs", ...).
    pub key: String,
    /// Friendly label ("Render", "Video", ...).
    pub label: String,
    pub utilization_pct: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GpuSample {
    pub card: u32,
    pub pci: String,
    pub driver: &'static str,
    pub model: String,
    pub integrated: bool,
    /// Aggregate utilization: max over engines (0..100).
    pub utilization_pct: f64,
    pub engines: Vec<EngineStat>,
    pub cur_mhz: Option<f64>,
    pub max_mhz: Option<f64>,
    pub min_mhz: Option<f64>,
    pub temps: Vec<hwmon::Temp>,
    /// Primary temperature: "pkg" if labeled, else hottest sensor.
    pub temp_c: Option<f64>,
    /// Power draw in watts from hwmon energy counter deltas.
    pub power_w: Option<f64>,
    /// Static power limit in watts (TDP).
    pub power_limit_w: Option<f64>,
    pub fan_rpm: Option<f64>,
    /// VRAM in use by clients (MiB) — discrete cards.
    pub vram_used_mb: Option<f64>,
    /// Shared/GTT memory in use by clients (MiB).
    pub shared_used_mb: Option<f64>,
    /// Total VRAM (MiB). xe only exposes this through a device query
    /// ioctl; roadmap item.
    pub vram_total_mb: Option<f64>,
}

// ---------------------------------------------------------------------
// fdinfo client accounting

/// Raw counters for one DRM client at one instant.
#[derive(Debug, Clone, Default)]
struct ClientCounters {
    /// i915 style: cumulative busy *nanoseconds* per engine.
    engines_ns: HashMap<String, u64>,
    /// xe style: cumulative busy *cycles* per engine.
    engines_cycles: HashMap<String, u64>,
    /// xe: total elapsed cycles per engine (denominator).
    engine_total_cycles: HashMap<String, u64>,
    /// HW instances per engine (xe); default 1.
    capacity: HashMap<String, u64>,
    vram_kib: u64,
    shared_kib: u64,
}

/// Per-PCI map of client-id -> counters.
type ClientMap = HashMap<String, HashMap<u64, ClientCounters>>;

// ---------------------------------------------------------------------
// Stateful collector

pub struct GpuState {
    cards: Vec<GpuCard>,
    prev_clients: Option<ClientMap>,
    prev_clients_at: Instant,
    /// per-PCI energy counters for power draw.
    prev_energy: HashMap<String, (Vec<u64>, Instant)>,
    /// hwmon dirs owned by GPUs (to exclude from system fans).
    gpu_hwmon_dirs: Vec<PathBuf>,
    /// One-shot collector notices drained by the sampler.
    pending_warnings: Vec<String>,
    xe_counter_warning_emitted: bool,
}

impl GpuState {
    pub fn new() -> Self {
        let cards = discover_cards();
        let gpu_hwmon_dirs = cards
            .iter()
            .flat_map(|c| c.hwmon_dirs.iter().cloned())
            .collect();
        Self {
            cards,
            prev_clients: None,
            prev_clients_at: Instant::now(),
            prev_energy: HashMap::new(),
            gpu_hwmon_dirs,
            pending_warnings: Vec::new(),
            xe_counter_warning_emitted: false,
        }
    }

    pub fn sample(&mut self) -> Vec<GpuSample> {
        let now = Instant::now();
        let clients = scan_fdinfo(&self.cards);
        let mut out = Vec::new();

        for card in &self.cards {
            let cur_map = clients.get(&card.pci);
            let prev_map = self.prev_clients.as_ref().and_then(|m| m.get(&card.pci));

            let engines = differ_engines(prev_map, cur_map, now, self.prev_clients_at);
            let utilization_pct = engines
                .iter()
                .map(|e| e.utilization_pct)
                .fold(0.0_f64, f64::max);

            let (cur_mhz, max_mhz, min_mhz) = read_clocks(card);

            // --- hwmon: temps / fan / limits / energy ------------------
            let mut temps = Vec::new();
            let mut fan_rpm = None;
            let mut power_limit_w = None;
            let mut energy: Vec<u64> = Vec::new();
            for d in &card.hwmon_dirs {
                let r = hwmon::readout(d);
                temps.extend(r.temps);
                if fan_rpm.is_none() {
                    fan_rpm = r.fans_rpm.first().copied();
                }
                if power_limit_w.is_none() {
                    power_limit_w = r.power_limit_w;
                }
                if energy.is_empty() {
                    energy = r.energy_uj;
                }
            }
            temps.sort_by(|a, b| a.label.cmp(&b.label));
            let temp_c = temps
                .iter()
                .find(|t| t.label.eq_ignore_ascii_case("pkg"))
                .map(|t| t.celsius)
                .or_else(|| {
                    temps
                        .iter()
                        .map(|t| t.celsius)
                        .fold(None, |acc: Option<f64>, c| {
                            Some(acc.map_or(c, |a| a.max(c)))
                        })
                });

            // --- power draw from energy deltas -------------------------
            // prev_energy stores the full counter vector (a hwmon device
            // may expose more than one); we difference the first counter.
            let power_w = match (self.prev_energy.get(&card.pci), energy.first()) {
                (Some((prev_vec, prev_t)), Some(&now_e)) => prev_vec.first().and_then(|&prev_e| {
                    (now_e >= prev_e)
                        .then(|| {
                            let du = now_e - prev_e;
                            let dt = now.duration_since(*prev_t).as_secs_f64();
                            (dt > 0.0).then(|| du as f64 / 1e6 / dt)
                        })
                        .flatten()
                }),
                _ => None,
            };
            if !energy.is_empty() {
                self.prev_energy.insert(card.pci.clone(), (energy, now));
            }

            // --- client memory ------------------------------------------
            let (vram_used_mb, shared_used_mb) = client_memory(cur_map);

            // --- missing-capability hint (xe cycles need CAP_PERFMON
            //     on some kernels) ----------------------------------------
            if card.driver == "xe"
                && !self.xe_counter_warning_emitted
                && cur_map.map(|m| !m.is_empty()).unwrap_or(false)
                && engines.is_empty()
            {
                self.xe_counter_warning_emitted = true;
                self.pending_warnings.push(
                    "xe utilization counters read zero — some kernels require \
                     CAP_PERFMON (see scripts/setup-permissions.sh)"
                        .into(),
                );
            }

            out.push(GpuSample {
                card: card.index,
                pci: card.pci.clone(),
                driver: card.driver,
                model: card.model.clone(),
                integrated: card.integrated,
                utilization_pct,
                engines,
                cur_mhz,
                max_mhz,
                min_mhz,
                temp_c,
                temps,
                power_w,
                power_limit_w,
                fan_rpm,
                vram_used_mb,
                shared_used_mb,
                vram_total_mb: None,
            });
        }

        self.prev_clients = Some(clients);
        self.prev_clients_at = now;
        out
    }

    /// Fans not owned by a GPU (motherboard / AIO).
    pub fn system_fans(&self) -> Vec<NamedFan> {
        let mut fans = Vec::new();
        for dir in hwmon::all() {
            if self.gpu_hwmon_dirs.iter().any(|g| same_path(g, &dir)) {
                continue;
            }
            let r = hwmon::readout(&dir);
            for (i, rpm) in r.fans_rpm.iter().enumerate() {
                fans.push(NamedFan {
                    label: format!("{} fan{}", hwmon::name(&dir), i + 1),
                    rpm: *rpm,
                });
            }
        }
        fans
    }

    /// Drain one-shot collector notices (e.g. missing capabilities).
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_warnings)
    }
}

// ---------------------------------------------------------------------
// Card discovery

fn discover_cards() -> Vec<GpuCard> {
    let mut cards = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/drm") else {
        return cards; // no DRM on this machine; UI will show a hint
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(index) = name
            .strip_prefix("card")
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        let sysfs = PathBuf::from(format!("/sys/class/drm/card{index}"));
        let device = sysfs.join("device");

        let uevent = fs::read_to_string(device.join("uevent")).unwrap_or_default();
        let driver = uevent
            .lines()
            .find_map(|l| l.strip_prefix("DRIVER="))
            .unwrap_or("");
        if driver != "i915" && driver != "xe" {
            continue; // not an Intel GPU — out of scope by design
        }
        let pci = uevent
            .lines()
            .find_map(|l| l.strip_prefix("PCI_SLOT_NAME="))
            .unwrap_or("")
            .to_string();
        let pci_id = uevent
            .lines()
            .find_map(|l| l.strip_prefix("PCI_ID=8086:"))
            .unwrap_or("");
        // pci.ids stores device IDs in lowercase; the kernel uevent
        // reports them in uppercase.
        let model = lookup_pci_name(&pci_id.to_lowercase())
            .unwrap_or_else(|| format!("Intel GPU {pci_id}"));

        let hwmon_dirs = hwmon::for_card(&device);
        let integrated = pci.starts_with("0000:00:");
        let render_minors = discover_render_minors(&device);

        cards.push(GpuCard {
            index,
            pci,
            driver: match driver {
                "i915" => "i915",
                _ => "xe",
            },
            model,
            integrated,
            sysfs,
            hwmon_dirs,
            render_minors,
        });
    }
    cards.sort_by_key(|c| c.index);
    cards
}

/// Find renderD minors whose PCI device matches `device`.
fn discover_render_minors(device: &Path) -> Vec<u32> {
    let mut minors = Vec::new();
    if let Ok(rd) = fs::read_dir("/sys/class/drm") {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(minor) = name
                .strip_prefix("renderD")
                .and_then(|n| n.parse::<u32>().ok())
            {
                if same_path(&e.path().join("device"), device) {
                    minors.push(minor);
                }
            }
        }
    }
    minors
}

fn same_path(a: &Path, b: &Path) -> bool {
    a == b
        || fs::canonicalize(a)
            .ok()
            .zip(fs::canonicalize(b).ok())
            .map(|(x, y)| x == y)
            .is_some()
}

/// Resolve an Intel device ID (e.g. "E223") to a marketing name using
/// the system pci.ids database. No external commands.
fn lookup_pci_name(dev_id: &str) -> Option<String> {
    const CANDIDATES: [&str; 4] = [
        "/usr/share/hwdata/pci.ids",
        "/usr/share/misc/pci.ids",
        "/usr/share/pci.ids",
        "/var/lib/pciutils/pci.ids",
    ];
    for path in CANDIDATES {
        let Ok(db) = fs::read_to_string(path) else {
            continue;
        };
        let mut in_vendor = false;
        for line in db.lines() {
            if line.starts_with('#') {
                continue; // file-wide comments appear mid-section too
            }
            if line.starts_with("8086  ") {
                in_vendor = true;
                continue;
            }
            if !in_vendor {
                continue;
            }
            if line.starts_with('\t') || line.starts_with("  ") {
                // Device line: "  E223  Battlemage G31 [Intel Graphics]"
                let trimmed = line.trim_start();
                if let Some((id, name)) = trimmed.split_once("  ") {
                    if id.trim() == dev_id {
                        return Some(name.trim().to_string());
                    }
                }
            } else if !line.trim().is_empty() {
                in_vendor = false; // next vendor section started
            }
        }
    }
    None
}

// ---------------------------------------------------------------------
// fdinfo scanning

fn scan_fdinfo(cards: &[GpuCard]) -> ClientMap {
    // minor -> PCI for fd-symlink fallback attribution
    let minor_to_pci: HashMap<u32, &str> = cards
        .iter()
        .flat_map(|c| c.render_minors.iter().map(move |m| (*m, c.pci.as_str())))
        .collect();

    let mut out: ClientMap = HashMap::new();

    let Ok(procs) = fs::read_dir("/proc") else {
        return out;
    };
    for p in procs.flatten() {
        let pid_dir = p.path();
        let fdinfo_dir = pid_dir.join("fdinfo");
        let Ok(fds) = fs::read_dir(&fdinfo_dir) else {
            continue;
        };
        for fd in fds.flatten() {
            let Ok(content) = fs::read_to_string(fd.path()) else {
                continue;
            };
            if !content.contains("drm-driver") {
                continue;
            }
            let mut pci: Option<String> = None;
            let mut client_id: Option<u64> = None;
            let mut counters = ClientCounters::default();

            for line in content.lines() {
                let Some((key, value)) = line.split_once(':') else {
                    continue;
                };
                let key = key.trim();
                let value = value.trim();
                let num = || {
                    value
                        .split_whitespace()
                        .next()
                        .and_then(|v| v.parse::<u64>().ok())
                };

                match key {
                    "drm-pdev" => pci = Some(value.to_string()),
                    "drm-client-id" => client_id = num(),
                    _ if key.starts_with("drm-engine-capacity-") => {
                        if let (Some(k), Some(c)) =
                            (key.strip_prefix("drm-engine-capacity-"), num())
                        {
                            counters.capacity.insert(k.to_string(), c);
                        }
                    }
                    _ if key.starts_with("drm-engine-") => {
                        // i915 cumulative busy nanoseconds
                        if let (Some(k), Some(v)) = (key.strip_prefix("drm-engine-"), num()) {
                            counters
                                .engines_ns
                                .entry(k.to_string())
                                .and_modify(|e| *e = (*e).max(v))
                                .or_insert(v);
                        }
                    }
                    _ if key.starts_with("drm-total-cycles-") => {
                        if let (Some(k), Some(v)) = (key.strip_prefix("drm-total-cycles-"), num()) {
                            counters
                                .engine_total_cycles
                                .entry(k.to_string())
                                .and_modify(|e| *e = (*e).max(v))
                                .or_insert(v);
                        }
                    }
                    _ if key.starts_with("drm-cycles-") => {
                        if let (Some(k), Some(v)) = (key.strip_prefix("drm-cycles-"), num()) {
                            counters
                                .engines_cycles
                                .entry(k.to_string())
                                .and_modify(|e| *e = (*e).max(v))
                                .or_insert(v);
                        }
                    }
                    "drm-total-vram0" | "drm-memory-local" => {
                        counters.vram_kib = counters.vram_kib.max(num().unwrap_or(0));
                    }
                    "drm-total-gtt" | "drm-memory-shared" => {
                        counters.shared_kib = counters.shared_kib.max(num().unwrap_or(0));
                    }
                    _ => {}
                }
            }

            // Attribution: prefer drm-pdev, fall back to the fd symlink.
            let pci = match pci {
                Some(p) => p,
                None => {
                    let fd_path = pid_dir.join("fd").join(fd.file_name());
                    let Ok(target) = fs::read_link(&fd_path) else {
                        continue;
                    };
                    let target = target.to_string_lossy();
                    let Some(minor) = target
                        .rsplit('/')
                        .next()
                        .and_then(|s| s.strip_prefix("renderD"))
                        .and_then(|s| s.parse::<u32>().ok())
                    else {
                        continue;
                    };
                    match minor_to_pci.get(&minor) {
                        Some(p) => p.to_string(),
                        None => continue,
                    }
                }
            };
            let Some(client_id) = client_id else { continue };

            out.entry(pci)
                .or_default()
                .entry(client_id)
                // Multiple FDs of one client report the same cumulative
                // counters: keep max, never sum.
                .and_modify(|existing| merge_max(existing, &counters))
                .or_insert(counters);
        }
    }
    out
}

fn merge_max(a: &mut ClientCounters, b: &ClientCounters) {
    for (k, v) in &b.engines_ns {
        a.engines_ns
            .entry(k.clone())
            .and_modify(|e| *e = (*e).max(*v))
            .or_insert(*v);
    }
    for (k, v) in &b.engines_cycles {
        a.engines_cycles
            .entry(k.clone())
            .and_modify(|e| *e = (*e).max(*v))
            .or_insert(*v);
    }
    for (k, v) in &b.engine_total_cycles {
        a.engine_total_cycles
            .entry(k.clone())
            .and_modify(|e| *e = (*e).max(*v))
            .or_insert(*v);
    }
    for (k, v) in &b.capacity {
        a.capacity
            .entry(k.clone())
            .and_modify(|e| *e = (*e).max(*v))
            .or_insert(*v);
    }
    a.vram_kib = a.vram_kib.max(b.vram_kib);
    a.shared_kib = a.shared_kib.max(b.shared_kib);
}

// ---------------------------------------------------------------------
// Differencing

fn differ_engines(
    prev: Option<&HashMap<u64, ClientCounters>>,
    cur: Option<&HashMap<u64, ClientCounters>>,
    now: Instant,
    prev_at: Instant,
) -> Vec<EngineStat> {
    let Some(cur) = cur else { return Vec::new() };
    let Some(prev) = prev else { return Vec::new() };
    let elapsed_ns = now.saturating_duration_since(prev_at).as_nanos() as f64;
    if elapsed_ns <= 0.0 {
        return Vec::new();
    }

    // key -> (busy_ns_delta, xe_busy_fraction, capacity)
    let mut acc: HashMap<String, (f64, f64, f64)> = HashMap::new();

    for (id, c) in cur {
        let p = prev.get(id);
        let touch = |k: &str, acc: &mut HashMap<String, (f64, f64, f64)>| {
            let cap = c.capacity.get(k).copied().unwrap_or(1).max(1) as f64;
            let e = acc.entry(k.to_string()).or_insert((0.0, 0.0, cap));
            if e.2 < cap {
                e.2 = cap;
            }
        };

        // i915: cumulative busy nanoseconds
        for (k, v) in &c.engines_ns {
            let d = v.saturating_sub(p.and_then(|pp| pp.engines_ns.get(k).copied()).unwrap_or(0));
            touch(k, &mut acc);
            acc.get_mut(k).unwrap().0 += d as f64;
        }

        // xe: busy cycles / total cycles per client
        for (k, busy) in &c.engines_cycles {
            let total = c.engine_total_cycles.get(k).copied().unwrap_or(0);
            let prev_busy = p
                .and_then(|pp| pp.engines_cycles.get(k).copied())
                .unwrap_or(0);
            let prev_total = p
                .and_then(|pp| pp.engine_total_cycles.get(k).copied())
                .unwrap_or(0);
            let d_total = total.saturating_sub(prev_total);
            if d_total > 0 {
                let frac = busy.saturating_sub(prev_busy) as f64 / d_total as f64;
                touch(k, &mut acc);
                acc.get_mut(k).unwrap().1 += frac;
            }
        }
    }

    let mut out: Vec<EngineStat> = acc
        .into_iter()
        .filter_map(|(key, (busy_ns, xe_frac, cap))| {
            // Prefer the cycle-accurate xe counters when present.
            let util = if xe_frac > 0.0 {
                xe_frac / cap * 100.0
            } else {
                busy_ns / (elapsed_ns * cap) * 100.0
            };
            let util = util.clamp(0.0, 100.0);
            (util > 0.01).then(|| EngineStat {
                key: key.clone(),
                label: engine_label(&key).to_string(),
                utilization_pct: util,
            })
        })
        .collect();
    out.sort_by(|a, b| b.utilization_pct.partial_cmp(&a.utilization_pct).unwrap());
    out
}

fn client_memory(cur: Option<&HashMap<u64, ClientCounters>>) -> (Option<f64>, Option<f64>) {
    let Some(cur) = cur else { return (None, None) };
    let mut vram = 0u64;
    let mut shared = 0u64;
    for c in cur.values() {
        vram += c.vram_kib;
        shared += c.shared_kib;
    }
    let vram_used_mb = (vram > 0).then(|| vram as f64 / 1024.0);
    let shared_used_mb = (shared > 0).then(|| shared as f64 / 1024.0);
    (vram_used_mb, shared_used_mb)
}

// ---------------------------------------------------------------------
// Clocks

fn read_clocks(card: &GpuCard) -> (Option<f64>, Option<f64>, Option<f64>) {
    match card.driver {
        "i915" => {
            let r = |f: &str| hwmon::read_f64(&card.sysfs.join(f));
            (
                r("gt_cur_freq_mhz"),
                r("gt_max_freq_mhz"),
                r("gt_min_freq_mhz"),
            )
        }
        _ => {
            // xe: device/tile*/gt*/freq0/{cur,max,min}_freq (MHz)
            let mut cur = None;
            let mut max = None;
            let mut min = None;
            if let Ok(dev) = fs::read_dir(card.sysfs.join("device")) {
                for tile in dev.flatten() {
                    if !tile.file_name().to_string_lossy().starts_with("tile") {
                        continue;
                    }
                    if let Ok(gts) = fs::read_dir(tile.path()) {
                        for gt in gts.flatten() {
                            let freq0 = gt.path().join("freq0");
                            if !freq0.is_dir() {
                                continue;
                            }
                            cur = max_opt(cur, hwmon::read_f64(&freq0.join("cur_freq")));
                            max = max_opt(max, hwmon::read_f64(&freq0.join("max_freq")));
                            min = min_opt(min, hwmon::read_f64(&freq0.join("min_freq")));
                        }
                    }
                }
            }
            (cur, max, min)
        }
    }
}

fn max_opt(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn min_opt(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}
