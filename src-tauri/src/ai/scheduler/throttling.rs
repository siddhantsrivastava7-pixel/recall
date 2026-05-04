//! Decides whether the worker should run right now.
//!
//! Honors three settings:
//!   * `ai.pause_on_battery` — pause when host is on battery (any level).
//!   * `ai.heavy_only_on_ac` — same effect today as pause_on_battery
//!     (kept distinct because Phase 1+ may classify some jobs as
//!     "light" and let them run on battery).
//!   * `ai.pause_below_battery_pct` (v0.5.22) — pause when battery
//!     percent drops below this threshold, regardless of AC state.
//!     Lets a user keep AI work running on battery for normal
//!     stretches but bail out when the laptop is genuinely low.
//!
//! AC + battery-percent detection are both best-effort
//! (see [`crate::ai::hardware::is_on_ac_power`] and
//! [`crate::ai::hardware::battery_percent`]). Unknown values are
//! treated as "no constraint to apply" so users on desktops without
//! battery sensors don't get the scheduler permanently parked.

use crate::ai::hardware;
use crate::db::repositories::SharedSettingsRepository;
use crate::errors::app_error::AppResult;

/// Return `true` if the worker should claim and run a job right now,
/// `false` if it should park (and re-evaluate when the scheduler is
/// notified next, or after a short timeout).
pub async fn can_run_now(settings: &SharedSettingsRepository) -> AppResult<bool> {
    let snapshot = settings.get().await?;
    if !snapshot.ai_enabled {
        return Ok(false);
    }

    // AC-power gate. `None` = unknown — assume on AC so we don't punish
    // desktop users.
    let on_ac = hardware::is_on_ac_power().unwrap_or(true);
    if !on_ac {
        if snapshot.ai_pause_on_battery {
            return Ok(false);
        }
        if snapshot.ai_heavy_only_on_ac {
            return Ok(false);
        }
    }

    // v0.5.22: low-battery gate. Independent from AC state — a laptop
    // plugged into a struggling charger can still drop in percent, and
    // we want to ease off then. `0` disables the gate; otherwise we
    // pause when the battery is reading below the user's threshold.
    // `None` from battery_percent (no battery sensor / macOS / desktop)
    // is interpreted as "nothing to pause for" — we'd rather run than
    // mistakenly park forever.
    if snapshot.ai_pause_below_battery_pct > 0 {
        if let Some(percent) = hardware::battery_percent() {
            if (percent as u32) < snapshot.ai_pause_below_battery_pct {
                return Ok(false);
            }
        }
    }

    Ok(true)
}
