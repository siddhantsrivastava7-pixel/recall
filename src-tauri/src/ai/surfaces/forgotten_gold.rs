//! Forgotten Gold picker — v0.5.23.
//!
//! Rule-based for v0.5.23. We deliberately don't depend on
//! embeddings being computed for everything (some users haven't run
//! the embed-all pass yet), so the candidate query is a SQL filter
//! that's correct without vectors:
//!
//!   * memory's `project_id` is non-null
//!   * the project has had ANY capture in the last 7 days
//!     (= the project is "active this week")
//!   * the memory itself is at least 14 days old (won't surface
//!     yesterday's stuff)
//!   * the memory hasn't been opened in 14 days (won't re-surface
//!     stuff the user just looked at)
//!   * it's not the recap memory itself (`source_app = 'spoken'`
//!     daily recap, weekly recap)
//!
//! Within the filtered set we order by `quality_score DESC` then
//! `created_at DESC` to prefer well-formed memories.
//!
//! When no active-project candidate exists, we fall back to the
//! same query without the project-activity gate — the user gets a
//! "your library has stuff from last month worth revisiting" card
//! instead of nothing.
//!
//! v0.5.24+ may add embedding-based clustering (cluster recent
//! activity, find old memories whose centroid is close), entity
//! overlap (memory mentions a person you talked about this week),
//! and explicit project briefings. The schema already supports it.

use chrono::{DateTime, Duration, Utc};
use sqlx::SqlitePool;

use crate::{errors::app_error::AppResult, models::Memory};

/// Configurable knobs. Constants here so they're documented but
/// not so rigid we can't change them later in this same file.
const MIN_AGE_DAYS: i64 = 14;
const UNOPENED_DAYS: i64 = 14;
const ACTIVE_PROJECT_WINDOW_DAYS: i64 = 7;
const MAX_AGE_DAYS_FALLBACK: i64 = 180; // Don't surface ancient stuff.

/// Result returned by the picker. Holds enough to render a card +
/// build the surface row; the surface engine fills in score and
/// reason from these fields.
#[derive(Debug, Clone)]
pub struct ForgottenGoldCandidate {
    pub memory: Memory,
    /// Score in 0..=1. Today this is just `quality_score / 100` if
    /// `quality_score` exists; else 0.5 by default. Future versions
    /// will mix in cluster-distance and recency signals.
    pub score: f64,
    /// Short, user-facing explanation rendered as the card subtitle.
    pub reason: String,
}

pub async fn pick(pool: &SqlitePool, now: DateTime<Utc>) -> AppResult<Option<ForgottenGoldCandidate>> {
    let min_age = (now - Duration::days(MIN_AGE_DAYS)).to_rfc3339();
    let max_age = (now - Duration::days(MAX_AGE_DAYS_FALLBACK)).to_rfc3339();
    let unopened_cutoff = (now - Duration::days(UNOPENED_DAYS)).to_rfc3339();
    let active_project_cutoff =
        (now - Duration::days(ACTIVE_PROJECT_WINDOW_DAYS)).to_rfc3339();

    // First pass: prefer memories whose project has activity this
    // week. The card reads "related to your <project> work this
    // week" which is the strongest reason we can give without
    // embeddings.
    if let Some(memory) = pick_active_project_candidate(
        pool,
        &min_age,
        &max_age,
        &unopened_cutoff,
        &active_project_cutoff,
    )
    .await?
    {
        let project_label = memory
            .project_name
            .clone()
            .unwrap_or_else(|| "this project".to_string());
        let age_phrase = age_phrase_from(&memory.created_at, now);
        let reason = format!(
            "Saved {age_phrase}. Related to your {project_label} work this week."
        );
        let score = score_from_quality(memory.quality_score, 0.65);
        return Ok(Some(ForgottenGoldCandidate {
            memory,
            score,
            reason,
        }));
    }

    // Fallback: any old, unopened, well-scored memory from the
    // user's library. Reason is softer — we don't have a project
    // hook to anchor on.
    if let Some(memory) =
        pick_fallback_candidate(pool, &min_age, &max_age, &unopened_cutoff).await?
    {
        let age_phrase = age_phrase_from(&memory.created_at, now);
        let reason = format!("Saved {age_phrase}. Worth a second look.");
        let score = score_from_quality(memory.quality_score, 0.45);
        return Ok(Some(ForgottenGoldCandidate {
            memory,
            score,
            reason,
        }));
    }

    Ok(None)
}

/// Active-project query. Excludes:
///   * recap memories (we'd be surfacing our own surfaces)
///   * Q&A memories saved by the user (`source_app = 'ask-recall'`)
///   * memories opened recently (last_opened_at within window)
///
/// Selects from a set of project_ids that have at least one memory
/// from the last `ACTIVE_PROJECT_WINDOW_DAYS` days. Ordered by
/// `quality_score DESC` (NULLs last) then `created_at DESC`.
async fn pick_active_project_candidate(
    pool: &SqlitePool,
    min_age: &str,
    max_age: &str,
    unopened_cutoff: &str,
    active_project_cutoff: &str,
) -> AppResult<Option<Memory>> {
    let select = crate::db::sqlite_memory_repository::MEMORY_SELECT;
    let sql = format!(
        "{select} \
         WHERE memories.project_id IS NOT NULL \
           AND memories.project_id IN ( \
             SELECT DISTINCT project_id FROM memories \
             WHERE project_id IS NOT NULL \
               AND datetime(created_at) >= datetime(?1) \
           ) \
           AND datetime(memories.created_at) <= datetime(?2) \
           AND datetime(memories.created_at) >= datetime(?3) \
           AND ( \
             memories.last_opened_at IS NULL \
             OR datetime(memories.last_opened_at) <= datetime(?4) \
           ) \
           AND COALESCE(memories.source_app, '') NOT IN ('spoken', 'weekly', 'ask-recall') \
           AND memories.id NOT IN ( \
             SELECT memory_id FROM proactive_surfaces \
             WHERE kind = 'forgotten_gold' \
               AND datetime(surfaced_at) >= datetime(?1) \
           ) \
         ORDER BY \
           CASE WHEN memories.quality_score IS NULL THEN 1 ELSE 0 END, \
           memories.quality_score DESC, \
           datetime(memories.created_at) DESC \
         LIMIT 1"
    );
    let row = sqlx::query_as::<_, Memory>(&sql)
        .bind(active_project_cutoff)
        .bind(min_age)
        .bind(max_age)
        .bind(unopened_cutoff)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// Fallback query without the active-project gate. Same exclusions
/// otherwise. Used when the user has zero active projects this
/// week (e.g. they're on vacation) — we still give them something
/// to revisit if their library has reasonable depth.
async fn pick_fallback_candidate(
    pool: &SqlitePool,
    min_age: &str,
    max_age: &str,
    unopened_cutoff: &str,
) -> AppResult<Option<Memory>> {
    let select = crate::db::sqlite_memory_repository::MEMORY_SELECT;
    let sql = format!(
        "{select} \
         WHERE datetime(memories.created_at) <= datetime(?1) \
           AND datetime(memories.created_at) >= datetime(?2) \
           AND ( \
             memories.last_opened_at IS NULL \
             OR datetime(memories.last_opened_at) <= datetime(?3) \
           ) \
           AND COALESCE(memories.source_app, '') NOT IN ('spoken', 'weekly', 'ask-recall') \
           AND memories.id NOT IN ( \
             SELECT memory_id FROM proactive_surfaces \
             WHERE kind = 'forgotten_gold' \
               AND datetime(surfaced_at) >= datetime(?2) \
           ) \
           AND COALESCE(memories.quality_score, 0) > 0 \
         ORDER BY \
           memories.quality_score DESC, \
           datetime(memories.created_at) DESC \
         LIMIT 1"
    );
    let row = sqlx::query_as::<_, Memory>(&sql)
        .bind(min_age)
        .bind(max_age)
        .bind(unopened_cutoff)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// Map a quality_score (0..=100) to a 0..=1 surface score. Falls
/// back to `default_score` when quality is unset. Strict bounds so
/// downstream code can reason about ranges.
fn score_from_quality(quality_score: Option<f64>, default_score: f64) -> f64 {
    match quality_score {
        Some(q) => (q / 100.0).clamp(0.0, 1.0),
        None => default_score,
    }
}

/// Human-readable phrase like "3 weeks ago" / "2 months ago" from
/// a UTC RFC3339 timestamp. Used in the surface card subtitle.
/// Coarse on purpose — "27 days ago" reads worse than "4 weeks ago"
/// in a card subtitle.
fn age_phrase_from(created_at: &str, now: DateTime<Utc>) -> String {
    let parsed = DateTime::parse_from_rfc3339(created_at)
        .map(|dt| dt.with_timezone(&Utc))
        .ok();
    let Some(parsed) = parsed else {
        return "a while ago".to_string();
    };
    let age = now - parsed;
    let days = age.num_days();
    if days < 0 {
        return "recently".to_string();
    }
    if days < 14 {
        return format!("{days} days ago");
    }
    if days < 60 {
        let weeks = days / 7;
        return format!("{weeks} weeks ago");
    }
    let months = days / 30;
    if months < 12 {
        return format!("{months} months ago");
    }
    let years = days / 365;
    format!("{years} year{} ago", if years == 1 { "" } else { "s" })
}
