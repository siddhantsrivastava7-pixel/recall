//! Weekly recap composer — v0.5.23.
//!
//! Each calendar week (Monday-Sunday in local time) gets ONE recap
//! memory, mirroring the Daily recap pattern from v0.5.18:
//!
//!   * `external_id = "weekly:YYYY-WW"` (ISO week)
//!   * `source_app = "weekly"` (separate namespace from "spoken"
//!     so daily-recap queries don't accidentally pull weekly rows)
//!   * `title = "Weekly recap · Apr 21 – Apr 27"`
//!   * Body is a sectioned roll-up of every memory captured in
//!     that week's window, grouped the same way Daily recap groups
//!     things (Screenshots / Bookmarks / From iPhone / Saved notes).
//!     The "Spoken" section becomes a count rather than the full
//!     transcript — pulling 7 days of in-body transcripts would
//!     blow out the LLM context.
//!
//! The LLM summary is generated lazily by the `generate_daily_recap_summary`
//! command (which already accepts any "recap-like" memory whose
//! `source_app` is in the recap namespace — extending that check to
//! include `'weekly'` happens in the same v0.5.23 patch).

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Utc, Weekday};

use crate::{
    db::repositories::SharedMemoryRepository,
    errors::app_error::AppResult,
    models::{Memory, MemoryInput, MemorySourceType},
};

const WEEKLY_SOURCE_APP: &str = "weekly";
const WEEKLY_EXTERNAL_ID_PREFIX: &str = "weekly:";

/// Compute the local-tz Monday 00:00 / next-Monday 00:00 boundary
/// pair for the calendar week containing `now_local`. Used by the
/// engine to bracket "this week's surface activity" (i.e. has the
/// user already seen this week's recap card?).
pub fn this_week_window(now_local: DateTime<Local>) -> (DateTime<Local>, DateTime<Local>) {
    let weekday = now_local.weekday();
    let days_since_monday = match weekday {
        Weekday::Mon => 0,
        Weekday::Tue => 1,
        Weekday::Wed => 2,
        Weekday::Thu => 3,
        Weekday::Fri => 4,
        Weekday::Sat => 5,
        Weekday::Sun => 6,
    };
    let monday_date = now_local.date_naive() - Duration::days(days_since_monday as i64);
    let monday_local = local_day_start(monday_date);
    let next_monday_local = monday_local + Duration::days(7);
    (monday_local, next_monday_local)
}

/// v0.5.24: window for the **previous** completed week — the one
/// the surface card actually summarizes. On Monday morning of
/// week N, the recap card shows week N-1's content; without this
/// shift the card would be empty for the first few days of every
/// new week (this week hasn't accumulated anything yet).
pub fn last_week_window(now_local: DateTime<Local>) -> (DateTime<Local>, DateTime<Local>) {
    let (this_monday, _) = this_week_window(now_local);
    let last_monday = this_monday - Duration::days(7);
    (last_monday, this_monday)
}

/// Build the `external_id` for the LAST completed week (the one
/// the recap card surfaces). v0.5.24 shifted from this-week to
/// last-week framing; the prefix stays the same so any v0.5.23
/// rows continue to round-trip cleanly through `is_weekly_recap`.
pub fn last_week_external_id(now_local: DateTime<Local>) -> String {
    let (last_monday_local, _) = last_week_window(now_local);
    let iso = last_monday_local.iso_week();
    format!("{}{}-{:02}", WEEKLY_EXTERNAL_ID_PREFIX, iso.year(), iso.week())
}

/// Title text for a recap memory whose week starts on `monday`.
/// Format: "Weekly recap · Apr 21 – Apr 27".
fn week_title(monday: DateTime<Local>, sunday: DateTime<Local>) -> String {
    format!(
        "Weekly recap · {} – {}",
        monday.format("%b %-d"),
        sunday.format("%b %-d")
    )
}

/// Ensure last week's weekly-recap memory exists. Returns the
/// memory — creating it from a fresh DB query when missing, or
/// reading the existing row otherwise. Idempotent, safe to call
/// from any surface engine entrypoint.
///
/// v0.5.24: shifted from this-week to last-week framing. The
/// surface card on Home is meant to answer "what did I do?" not
/// "what am I doing?" — and on Monday morning of a new week the
/// "this week" window is necessarily empty (the week just
/// started). Showing last week's completed roll-up gives the
/// user useful context the moment they open the app.
///
/// `Ok(None)` only when last week has zero captures across any
/// source — there's nothing to recap, no card should render.
pub async fn ensure_recap_for_last_week(
    repo: &SharedMemoryRepository,
) -> AppResult<Option<Memory>> {
    let now_utc = Utc::now();
    let now_local = now_utc.with_timezone(&Local);
    let (monday_local, next_monday_local) = last_week_window(now_local);
    let sunday_local = next_monday_local - Duration::seconds(1);
    let external_id = last_week_external_id(now_local);
    let title = week_title(monday_local, sunday_local);

    let start_utc = monday_local.with_timezone(&Utc).to_rfc3339();
    let end_utc = next_monday_local.with_timezone(&Utc).to_rfc3339();
    let week_memories = repo.list_memories_for_day(&start_utc, &end_utc).await?;

    // No captures last week → no recap. Don't create an empty
    // memory; the surface engine will skip the weekly slot and
    // the next surface kind (Forgotten Gold) takes over.
    if week_memories.is_empty() {
        return Ok(None);
    }

    let body = compose_weekly_body(&monday_local, &sunday_local, &week_memories);

    if let Some(existing) = repo
        .find_by_external_source(WEEKLY_SOURCE_APP, &external_id)
        .await?
    {
        // Rebuild the body if the week's content changed since
        // last update. Preserves the row's id (and therefore any
        // cached AI summary; the renderer will detect staleness
        // via `updated_at > ai_summary_generated_at` and offer
        // regenerate).
        if existing.content == body {
            return Ok(Some(existing));
        }
        let updated = repo
            .update(
                &existing.id,
                MemoryInput {
                    source_type: Some(MemorySourceType::Manual),
                    title: Some(title),
                    content: body,
                    note: existing.note.clone(),
                    project_id: existing.project_id.clone(),
                    url: None,
                    external_id: Some(external_id),
                    folder_path: None,
                    source_app: Some(WEEKLY_SOURCE_APP.to_string()),
                    source_window: None,
                    created_at: Some(existing.created_at),
                    updated_at: Some(now_utc.to_rfc3339()),
                },
            )
            .await?;
        return Ok(Some(updated));
    }

    let created = repo
        .create(MemoryInput {
            source_type: Some(MemorySourceType::Manual),
            title: Some(title),
            content: body,
            note: None,
            project_id: None,
            url: None,
            external_id: Some(external_id),
            folder_path: None,
            source_app: Some(WEEKLY_SOURCE_APP.to_string()),
            source_window: None,
            created_at: Some(now_utc.to_rfc3339()),
            updated_at: Some(now_utc.to_rfc3339()),
        })
        .await?;
    Ok(Some(created))
}

/// Compose the weekly recap body. Mirrors Daily recap's section
/// layout (Summary header + per-source sections) but spans 7 days
/// and skips the in-body Spoken transcript — pulling 7 days of
/// snippets would blow out the LLM context. The Spoken count is
/// surfaced in the rule-based summary line so the weekly recap
/// still acknowledges spoken activity.
fn compose_weekly_body(
    monday: &DateTime<Local>,
    sunday: &DateTime<Local>,
    week_memories: &[Memory],
) -> String {
    let mut counts: std::collections::BTreeMap<&'static str, usize> =
        std::collections::BTreeMap::new();
    let mut by_section: std::collections::BTreeMap<&'static str, Vec<&Memory>> =
        std::collections::BTreeMap::new();

    for memory in week_memories {
        let bucket = section_for_memory(memory);
        *counts.entry(bucket).or_insert(0) += 1;
        by_section.entry(bucket).or_default().push(memory);
    }

    let header = format!(
        "Weekly recap · {} – {}",
        monday.format("%b %-d"),
        sunday.format("%b %-d")
    );
    let summary = format!(
        "Summary\n- {} memories captured between {} and {}.\n- {}",
        week_memories.len(),
        monday.format("%a %b %-d"),
        sunday.format("%a %b %-d"),
        format_count_breakdown(&counts)
    );

    let mut sections: Vec<String> = vec![summary];

    // Order matches Daily recap's section order so users see the
    // same layout language. v0.5.27: switched bullet builder to
    // `filter_map` because some memories don't have a useful
    // bullet line (e.g. a screenshot whose OCR hasn't completed —
    // its body is just placeholder text). The section header
    // always reflects the FULL count from `items.len()`, even
    // when individual bullets get dropped — counts stay
    // accurate, only the per-bullet noise gets filtered.
    for label in ["Screenshots", "Bookmarks", "From iPhone", "Saved notes"] {
        let Some(items) = by_section.get(label) else {
            continue;
        };
        if items.is_empty() {
            continue;
        }
        let total_count = items.len();
        let lines: Vec<String> = items
            .iter()
            .filter_map(|m| format_memory_bullet(m))
            .take(6)
            .collect();
        let shown = lines.len();
        let more = if total_count > shown {
            format!("\n- … and {} more", total_count - shown)
        } else {
            String::new()
        };
        // Skip the section entirely if every item filtered out
        // (e.g. all screenshots pre-OCR) AND the section
        // wouldn't otherwise add information beyond the count
        // line in Summary.
        if shown == 0 {
            sections.push(format!(
                "{label} ({total_count})\n- (details pending — OCR or enrichment still running)"
            ));
            continue;
        }
        sections.push(format!(
            "{label} ({total_count})\n{}{}",
            lines.join("\n"),
            more
        ));
    }

    if let Some(spoken_count) = counts.get("Spoken") {
        sections.push(format!(
            "Spoken ({spoken_count})\n- See individual Daily recap memories for transcripts."
        ));
    }

    format!("{header}\n\n{}", sections.join("\n\n"))
}

/// Categorize a memory the same way Daily recap does. Mirrors
/// `services::spoken_transcript_service::group_other_memories`
/// in shape but inlined here so the surfaces module doesn't pull
/// in a service-layer dep.
fn section_for_memory(memory: &Memory) -> &'static str {
    if memory.source_app.as_deref() == Some("spoken") {
        return "Spoken";
    }
    if matches!(memory.source_type, MemorySourceType::Bookmark) {
        return "Bookmarks";
    }
    match memory.source_app.as_deref() {
        Some("screenshot") => "Screenshots",
        Some("mobile") => "From iPhone",
        _ => "Saved notes",
    }
}

/// Render a single bullet for the per-section list. Returns None
/// when the memory has nothing useful to show in a recap context
/// (most commonly: a screenshot whose OCR hasn't completed yet,
/// whose preview text is just placeholder boilerplate). The
/// section header keeps the FULL count even when bullets get
/// dropped — accuracy over completeness.
///
/// v0.5.27 polish:
///   * Screenshots: prefer OCR text over the placeholder body;
///     skip entirely if OCR isn't `done` yet.
///   * Title/preview dedup: when the preview is essentially the
///     title with truncation noise, skip the preview half so the
///     bullet doesn't show "Foo bar baz" — "Foo bar baz…".
fn format_memory_bullet(memory: &Memory) -> Option<String> {
    let title = memory
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .or_else(|| {
            memory
                .resolved_title
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
        })
        .unwrap_or("Untitled")
        .to_string();
    let date = DateTime::parse_from_rfc3339(&memory.created_at)
        .map(|dt| dt.with_timezone(&Local).format("%a").to_string())
        .unwrap_or_else(|_| "—".to_string());

    let is_screenshot = memory.source_app.as_deref() == Some("screenshot");
    let raw_preview = if is_screenshot {
        // Pre-OCR screenshots: skip from the bullet list. Their
        // preview is just `Screenshot from clipboard (W×H). OCR
        // will fill in the text once it runs.` — pure noise in
        // a recap. They still count toward the section total.
        let ocr_done = memory.ocr_status.as_deref() == Some("done");
        if !ocr_done {
            return None;
        }
        memory
            .ocr_text
            .as_deref()
            .or(memory.summary_text.as_deref())
            .or(memory.preview_text.as_deref())
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    } else {
        memory
            .summary_text
            .as_deref()
            .or(memory.preview_text.as_deref())
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };

    let preview = truncate_chars(&raw_preview, 110);
    let preview = if is_redundant_with_title(&title, &preview) {
        String::new()
    } else {
        preview
    };

    if preview.is_empty() {
        Some(format!("- {date} · \"{title}\""))
    } else {
        Some(format!("- {date} · \"{title}\" — {preview}"))
    }
}

/// True when the preview text is essentially the same as the
/// title — i.e. one is a prefix of the other (modulo truncation
/// ellipses). Skips the preview side of the bullet so the user
/// doesn't see "Foo bar" — "Foo bar…" twice on the same line.
/// Comparison is case-sensitive (titles are typically already
/// case-normalized) and ignores trailing ellipses on either side.
fn is_redundant_with_title(title: &str, preview: &str) -> bool {
    if preview.is_empty() {
        return false;
    }
    let t = title.trim().trim_end_matches('…').trim_end_matches("...");
    let p = preview.trim().trim_end_matches('…').trim_end_matches("...");
    if t.is_empty() || p.is_empty() {
        return false;
    }
    if t == p {
        return true;
    }
    // Either side a prefix of the other (handles
    // "Foo bar baz quux" title vs "Foo bar baz qu…" preview).
    let shorter_len = t.len().min(p.len());
    if shorter_len < 20 {
        // For short titles, exact match only — avoids dropping
        // a real preview just because it shares a prefix with a
        // 12-char title.
        return false;
    }
    t.starts_with(p) || p.starts_with(t)
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let total = text.chars().count();
    if total <= max_chars {
        return text.to_string();
    }
    let kept: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

fn format_count_breakdown(counts: &std::collections::BTreeMap<&'static str, usize>) -> String {
    if counts.is_empty() {
        return "Nothing captured this week.".to_string();
    }
    let mut parts: Vec<String> = Vec::new();
    for (label, count) in counts {
        let phrase = match *label {
            "Screenshots" => format!("{count} screenshot{}", if *count == 1 { "" } else { "s" }),
            "Bookmarks" => format!("{count} bookmark{}", if *count == 1 { "" } else { "s" }),
            "From iPhone" => format!("{count} from iPhone"),
            "Saved notes" => format!("{count} note{}", if *count == 1 { "" } else { "s" }),
            "Spoken" => format!("{count} spoken transcript{}", if *count == 1 { "" } else { "s" }),
            other => format!("{count} {other}"),
        };
        parts.push(phrase);
    }
    parts.join(", ")
}

/// True when this memory is a weekly-recap memory. Used by the
/// generate_daily_recap_summary command to recognize weekly recaps
/// alongside daily recaps as valid summary targets.
pub fn is_weekly_recap(memory: &Memory) -> bool {
    memory.source_app.as_deref() == Some(WEEKLY_SOURCE_APP)
        && memory
            .external_id
            .as_deref()
            .map(|id| id.starts_with(WEEKLY_EXTERNAL_ID_PREFIX))
            .unwrap_or(false)
}

/// Local-tz midnight of a NaiveDate. Mirrors the helper in
/// spoken_transcript_service; duplicated here rather than imported
/// to keep the surfaces module's dependencies clean.
fn local_day_start(date: NaiveDate) -> DateTime<Local> {
    let naive = date
        .and_hms_opt(0, 0, 0)
        .expect("hms 0,0,0 always valid");
    Local
        .from_local_datetime(&naive)
        .earliest()
        .unwrap_or_else(|| Utc.from_utc_datetime(&naive).with_timezone(&Local))
}

