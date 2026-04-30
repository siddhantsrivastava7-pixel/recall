//! Decides whether the worker should run right now.
//!
//! Phase 1 honors two settings: `ai.pause_on_battery` and `ai.heavy_only_on_ac`.
//! AC detection is best-effort (see [`crate::ai::hardware::is_on_ac_power`]).
//! When AC state is unknown we treat the host as "on AC" so users on
//! desktops without battery sensors don't get the scheduler permanently
//! parked.

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

    Ok(true)
}
