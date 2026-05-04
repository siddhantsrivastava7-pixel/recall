//! Selection engine for proactive surfaces — v0.5.23.
//!
//! Given the user's current state, decide which ONE surface card
//! (if any) belongs at the top of Home right now. Strict singular:
//! never returns more than one — the product rule for v0.5.23 is
//! "premium over noisy."
//!
//! Selection priority:
//!   1. **Weekly recap** if today is Monday OR this is the user's
//!      first Home open of the new week (no surface row of kind
//!      `'weekly_recap'` recorded since this week's Monday) AND
//!      the week has at least one capture.
//!   2. **Forgotten Gold** otherwise, when a candidate exists.
//!   3. None.
//!
//! Within each kind we cache. Once Forgotten Gold is recorded for a
//! given local day, we re-show the same surface row for the rest
//! of that day rather than picking a fresh memory each refresh.
//! That matches the "once per day" cadence locked in the v0.5.23
//! product spec.
//!
//! The engine never silently re-creates a dismissed card. Dismissal
//! is the user saying "not this one"; we don't fight that.

use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone, Utc};
use serde::Serialize;

use crate::{
    ai::surfaces::{forgotten_gold, weekly_recap},
    db::repositories::{SharedMemoryRepository, SharedProactiveSurfaceRepository},
    errors::app_error::AppResult,
    models::{Memory, ProactiveSurfaceRow},
};

/// Payload returned to the frontend. Holds the surface row + the
/// underlying memory hydrated so the card can render without a
/// separate fetch round-trip.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveSurface {
    pub surface: ProactiveSurfaceRow,
    pub memory: Memory,
}

/// Compute (or fetch the cached) active surface card for right now.
/// Order: try Weekly recap first (highest priority), fall through
/// to Forgotten Gold. Returns `None` when neither qualifies — the
/// Home slot stays hidden.
///
/// Idempotent. Calling this twice on the same day with the same
/// underlying state returns the same `surface.id` for the recorded
/// kind — we cache the row and don't re-pick.
pub async fn compute_active_surface(
    pool: &sqlx::SqlitePool,
    memory_repo: &SharedMemoryRepository,
    surface_repo: &SharedProactiveSurfaceRepository,
) -> AppResult<Option<ActiveSurface>> {
    let now_utc = Utc::now();
    let now_local = now_utc.with_timezone(&Local);

    // 1. Weekly recap takes priority on Monday and on first
    // open of a new week. The "first open" check is a count of
    // surface rows of kind `'weekly_recap'` recorded since this
    // week's Monday — zero rows = haven't surfaced yet.
    if let Some(active) =
        weekly_recap_active(memory_repo, surface_repo, now_utc, now_local).await?
    {
        return Ok(Some(active));
    }

    // 2. Forgotten Gold — once-per-local-day cadence. If we already
    // recorded a forgotten_gold row for today, return that exact
    // row so the user sees the same memory throughout the day.
    if let Some(active) =
        forgotten_gold_active(pool, memory_repo, surface_repo, now_utc, now_local).await?
    {
        return Ok(Some(active));
    }

    Ok(None)
}

async fn weekly_recap_active(
    memory_repo: &SharedMemoryRepository,
    surface_repo: &SharedProactiveSurfaceRepository,
    now_utc: DateTime<Utc>,
    now_local: DateTime<Local>,
) -> AppResult<Option<ActiveSurface>> {
    // The surface card surfaces LAST week's completed recap (see
    // `weekly_recap::ensure_recap_for_last_week` for the rationale —
    // tl;dr "this week from Monday" is empty Monday morning, which
    // is exactly when users open the app expecting to see what they
    // did last week).
    //
    // The "have we surfaced this card yet?" gate is keyed off
    // THIS week's Monday, not last week's. Each new calendar week
    // gets one chance to surface last week's recap; once shown
    // (and possibly dismissed) we stay quiet until the next
    // calendar week rolls over.
    let (this_week_monday_local, this_week_next_monday_local) =
        weekly_recap::this_week_window(now_local);
    let this_week_monday_iso = this_week_monday_local.with_timezone(&Utc).to_rfc3339();

    let now_iso = now_utc.to_rfc3339();

    // If we've already recorded a card for this calendar week and
    // it's still active (not dismissed, not expired), use it.
    if let Some(existing) = surface_repo
        .latest_active_for_kind("weekly_recap", &now_iso)
        .await?
    {
        if existing.surfaced_at >= this_week_monday_iso {
            if let Some(memory) = memory_repo.find(&existing.memory_id).await? {
                return Ok(Some(ActiveSurface {
                    surface: existing,
                    memory,
                }));
            }
        }
    }

    // If we recorded a card already this week but it's no longer
    // active, the user dismissed it. Dismissal sticks for the rest
    // of the calendar week — we don't re-pick after the user said
    // "not this one." Next Monday reopens the slot.
    let already_recorded = surface_repo
        .has_recorded_since("weekly_recap", &this_week_monday_iso)
        .await?;
    if already_recorded {
        return Ok(None);
    }

    // First Home open of the calendar week. Pick last week's recap
    // memory — composing it on the fly if we haven't seen this
    // week-key before. Returns None when last week was empty
    // (fresh installs, or genuinely quiet weeks); engine then
    // falls through to Forgotten Gold.
    let memory = match weekly_recap::ensure_recap_for_last_week(memory_repo).await? {
        Some(memory) => memory,
        None => return Ok(None),
    };

    let (last_monday_local, last_next_monday_local) = weekly_recap::last_week_window(now_local);
    let reason = format!(
        "Your week from {} to {}.",
        last_monday_local.format("%b %-d"),
        (last_next_monday_local - chrono::Duration::seconds(1)).format("%b %-d")
    );
    // Card auto-expires when next calendar week starts — past that
    // point we want to surface a different last-week memory.
    let expires_at = this_week_next_monday_local.with_timezone(&Utc).to_rfc3339();

    let surface_id = surface_repo
        .record_surface(
            "weekly_recap",
            &memory.id,
            // Weekly recap always wins when conditions match — its
            // "score" is fixed at 1.0 for ordering against Forgotten
            // Gold (which scores in 0..=1 from quality).
            1.0,
            Some(&reason),
            &now_utc.to_rfc3339(),
            Some(&expires_at),
        )
        .await?;
    let surface = ProactiveSurfaceRow {
        id: surface_id,
        kind: "weekly_recap".to_string(),
        memory_id: memory.id.clone(),
        score: 1.0,
        reason: Some(reason),
        surfaced_at: now_utc.to_rfc3339(),
        dismissed_at: None,
        expires_at: Some(expires_at),
    };
    Ok(Some(ActiveSurface { surface, memory }))
}

async fn forgotten_gold_active(
    pool: &sqlx::SqlitePool,
    memory_repo: &SharedMemoryRepository,
    surface_repo: &SharedProactiveSurfaceRepository,
    now_utc: DateTime<Utc>,
    now_local: DateTime<Local>,
) -> AppResult<Option<ActiveSurface>> {
    // Once-per-day cadence: if today already has an active row,
    // re-render it instead of picking a fresh memory. The
    // `dismissed_at IS NULL` filter on `latest_active_for_kind`
    // means dismissed rows are correctly skipped — we don't
    // re-surface the same dismissed memory later in the day.
    let day_start_local = local_day_start(now_local.date_naive());
    let day_start_iso = day_start_local.with_timezone(&Utc).to_rfc3339();

    if let Some(existing) = surface_repo
        .latest_active_for_kind("forgotten_gold", &now_utc.to_rfc3339())
        .await?
    {
        if existing.surfaced_at >= day_start_iso {
            if let Some(memory) = memory_repo.find(&existing.memory_id).await? {
                return Ok(Some(ActiveSurface {
                    surface: existing,
                    memory,
                }));
            }
        }
    }

    // No active row for today (either dismissed or never picked).
    // If dismissed, leave the slot empty for the rest of today —
    // the user said "not this one"; we don't immediately try a
    // different memory.
    let already_dismissed_today = surface_repo
        .has_recorded_since("forgotten_gold", &day_start_iso)
        .await?;
    if already_dismissed_today {
        // Already picked once today. The latest_active check above
        // would have returned it if it were still active; if we're
        // here, the user dismissed it. Stay quiet for the day.
        return Ok(None);
    }

    let candidate = match forgotten_gold::pick(pool, now_utc).await? {
        Some(c) => c,
        None => return Ok(None),
    };

    // Forgotten gold expires at end-of-day local time. After
    // midnight, the engine picks a fresh candidate.
    let expires_at = (day_start_local + Duration::days(1))
        .with_timezone(&Utc)
        .to_rfc3339();

    let surface_id = surface_repo
        .record_surface(
            "forgotten_gold",
            &candidate.memory.id,
            candidate.score,
            Some(&candidate.reason),
            &now_utc.to_rfc3339(),
            Some(&expires_at),
        )
        .await?;
    let surface = ProactiveSurfaceRow {
        id: surface_id,
        kind: "forgotten_gold".to_string(),
        memory_id: candidate.memory.id.clone(),
        score: candidate.score,
        reason: Some(candidate.reason),
        surfaced_at: now_utc.to_rfc3339(),
        dismissed_at: None,
        expires_at: Some(expires_at),
    };
    Ok(Some(ActiveSurface {
        surface,
        memory: candidate.memory,
    }))
}

fn local_day_start(date: NaiveDate) -> DateTime<Local> {
    let naive = date
        .and_hms_opt(0, 0, 0)
        .expect("hms 0,0,0 always valid");
    Local
        .from_local_datetime(&naive)
        .earliest()
        .unwrap_or_else(|| Utc.from_utc_datetime(&naive).with_timezone(&Local))
}
