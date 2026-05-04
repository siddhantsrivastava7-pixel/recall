//! Hardware tier detection.
//!
//! Recall's AI subsystem adapts model selection and concurrency to the host
//! machine. Three coarse tiers cover the realistic install base:
//!
//! | Tier | RAM       | Phase 1 OCR concurrency |
//! |------|-----------|--------------------------|
//! |  A   | < 12 GB   | 1                        |
//! |  B   | 12 – 24 GB| 2                        |
//! |  C   | ≥ 24 GB   | 2                        |
//!
//! Phase 1 only uses the tier to pick OCR concurrency; Phase 2+ will use it
//! to select embedding/LLM model variants. The tier is auto-detected on
//! startup, surfaced read-only in Settings, and can be overridden via the
//! `ai.hardware_tier_override` setting once that ships in Phase 2.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// Coarse RAM-driven hardware classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HardwareTier {
    A,
    B,
    C,
}

impl HardwareTier {
    /// Maximum number of concurrent OCR jobs the scheduler will run on this
    /// tier. The PRD locks tier A to 1 (an 8 GB MacBook should never be
    /// thrashing two Vision threads at once) and tier B/C to 2.
    pub fn max_ocr_jobs(self) -> usize {
        match self {
            HardwareTier::A => 1,
            HardwareTier::B | HardwareTier::C => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            HardwareTier::A => "A",
            HardwareTier::B => "B",
            HardwareTier::C => "C",
        }
    }
}

/// Snapshot of detected hardware. Built once at app start and held on
/// [`crate::state::app_state::AppState`]; the AC-power flag is the only
/// field that can move during a session and is queried lazily.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareInfo {
    pub tier: HardwareTier,
    pub total_ram_bytes: u64,
    pub cpu_cores: usize,
    pub arch: CpuArch,
    pub os: OsKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CpuArch {
    AppleSilicon,
    X86_64,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OsKind {
    MacOs,
    Windows,
    Other,
}

const GB: u64 = 1024 * 1024 * 1024;

fn detect_tier(total_ram_bytes: u64) -> HardwareTier {
    // Thresholds are intentionally a hair under the round number so a
    // machine reporting "16 GB" via slightly-less-than-16 (kernel reserved
    // pages, etc.) still classifies as tier B.
    if total_ram_bytes < 12 * GB {
        HardwareTier::A
    } else if total_ram_bytes < 24 * GB {
        HardwareTier::B
    } else {
        HardwareTier::C
    }
}

fn detect_arch() -> CpuArch {
    // `target_arch` is resolved at compile time, which is correct for
    // Recall: we ship per-platform binaries so the running binary's arch is
    // the host arch.
    match std::env::consts::ARCH {
        "aarch64" if cfg!(target_os = "macos") => CpuArch::AppleSilicon,
        "x86_64" => CpuArch::X86_64,
        _ => CpuArch::Other,
    }
}

fn detect_os() -> OsKind {
    if cfg!(target_os = "macos") {
        OsKind::MacOs
    } else if cfg!(target_os = "windows") {
        OsKind::Windows
    } else {
        OsKind::Other
    }
}

static CACHED_INFO: OnceLock<HardwareInfo> = OnceLock::new();

/// Detect once and cache for the lifetime of the process. RAM/cores/arch
/// don't change at runtime; the AC-state flag is queried separately when
/// needed (see [`is_on_ac_power`]).
pub fn detect() -> HardwareInfo {
    CACHED_INFO
        .get_or_init(|| {
            let mut system = sysinfo::System::new();
            system.refresh_memory();
            system.refresh_cpu_list(sysinfo::CpuRefreshKind::new());

            let total_ram_bytes = system.total_memory();
            let cpu_cores = system.cpus().len().max(1);

            HardwareInfo {
                tier: detect_tier(total_ram_bytes),
                total_ram_bytes,
                cpu_cores,
                arch: detect_arch(),
                os: detect_os(),
            }
        })
        .clone()
}

/// Best-effort AC-power detection. Returns `None` when unknown — callers
/// should treat unknown as "assume on AC" so we don't accidentally pause
/// background work on desktops that don't expose battery state at all.
///
/// v0.5.22: Windows now wires `GetSystemPowerStatus`. macOS still stubs
/// to `None` — IOPSCopyPowerSourcesInfo binding isn't covered by the
/// objc2-* crates we use today and adding an IOKit shim is more work
/// than the current user base on Mac justifies.
pub fn is_on_ac_power() -> Option<bool> {
    power_status().and_then(|s| s.on_ac)
}

/// v0.5.22: best-effort battery percent (0-100). Returns `None` when
/// unknown (desktop with no battery, macOS, or transient API failure).
/// The throttling layer treats `None` as "no battery to worry about"
/// — the low-battery pause only kicks in when we have a real reading
/// below the user's threshold.
pub fn battery_percent() -> Option<u8> {
    power_status().and_then(|s| s.percent)
}

/// Internal: combined power-status snapshot. Both `on_ac` and `percent`
/// share the same OS call on Windows (`GetSystemPowerStatus`), so
/// reading them together avoids two syscalls when the throttling layer
/// checks both per worker tick.
#[derive(Debug, Clone, Copy, Default)]
struct PowerStatus {
    on_ac: Option<bool>,
    percent: Option<u8>,
}

#[cfg(target_os = "windows")]
fn power_status() -> Option<PowerStatus> {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut status = SYSTEM_POWER_STATUS::default();
    // Safety: GetSystemPowerStatus only writes into the provided struct.
    // It returns FALSE on failure, in which case we leave the snapshot
    // empty and let callers treat as "unknown."
    let ok = unsafe { GetSystemPowerStatus(&mut status as *mut _) };
    if ok.is_err() {
        return None;
    }
    // ACLineStatus: 0 = offline (battery), 1 = online (AC), 255 = unknown.
    let on_ac = match status.ACLineStatus {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    };
    // BatteryLifePercent: 0-100, or 255 = unknown. Some desktops with
    // no battery report 255 here and 1 in ACLineStatus — that's fine,
    // we surface percent as None and the throttling layer ignores it.
    let percent = match status.BatteryLifePercent {
        0..=100 => Some(status.BatteryLifePercent),
        _ => None,
    };
    Some(PowerStatus { on_ac, percent })
}

#[cfg(not(target_os = "windows"))]
fn power_status() -> Option<PowerStatus> {
    // macOS / other: no implementation today — see is_on_ac_power's
    // doc comment for context. Returning None means "unknown" which
    // the throttling layer maps to "assume on AC, no low-battery
    // pause" — same behavior as pre-v0.5.22.
    None
}
