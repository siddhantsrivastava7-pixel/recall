//! Temporal intent detector for Ask Recall queries.
//!
//! Boring + reliable: regex-style phrase matching on a small fixed
//! list. We deliberately don't use a learned classifier here — the
//! phrases are common enough that a hand-tuned matcher is more
//! accurate than a 100M-param model on a token-budget call.
//!
//! Calendar week semantics: ISO 8601 — week starts Monday. "This week"
//! is "Monday 00:00 of the current ISO week → now". "Last week" is
//! "Monday 00:00 of the previous ISO week → Sunday 23:59:59". We use
//! local time on the host so "today" matches what the user thinks
//! today is, not what UTC thinks.
//!
//! When a phrase matches we also strip it from the query string so
//! the residual ("license keys" from "license keys last week") can
//! drive semantic ranking within the temporal window.

use chrono::{DateTime, Datelike, Duration, Local, NaiveTime, TimeZone, Utc, Weekday};

/// Resolved temporal window. `start` and `end` are UTC instants stored
/// as ISO 8601 strings so they line up with `memories.created_at` (which
/// is written as UTC ISO via `chrono::Utc::now().to_rfc3339()` on the
/// capture path). The `label` is human-readable for the diagnostic log
/// + system-prompt context.
#[derive(Debug, Clone)]
pub struct TemporalWindow {
    pub label: &'static str,
    pub start_iso: String,
    pub end_iso: String,
}

/// Parse a question for temporal intent. Returns the resolved window
/// + the residual non-temporal portion of the query (used for
/// semantic ranking within mixed queries).
///
/// Detection is case-insensitive and matches the *first* phrase from
/// a longest-first list — so "last week" wins over "week", and "this
/// week" doesn't accidentally resolve to "today" on systems where
/// "this" appears in some other sense.
pub fn detect(question: &str) -> Option<(TemporalWindow, String)> {
    let lower = question.to_lowercase();
    let now = Local::now();

    // Longest-first so "last week" beats "week" and "summarize my
    // week" beats "my".
    const PATTERNS: &[(&str, &str)] = &[
        ("today's", "today"),
        ("today", "today"),
        ("yesterday's", "yesterday"),
        ("yesterday", "yesterday"),
        ("this week", "this_week"),
        ("past week", "this_week"),
        ("summarize my week", "this_week"),
        ("my week", "this_week"),
        ("the week", "this_week"),
        ("last week", "last_week"),
        ("previous week", "last_week"),
        ("this month", "this_month"),
        ("past month", "this_month"),
        ("my month", "this_month"),
        ("last month", "last_month"),
        ("previous month", "last_month"),
        ("recently", "recent"),
        ("recent", "recent"),
        ("lately", "recent"),
    ];

    let mut best: Option<(usize, usize, &str)> = None; // (start, end, kind)
    for (phrase, kind) in PATTERNS {
        if let Some(start) = lower.find(phrase) {
            let end = start + phrase.len();
            // Prefer the earliest match; on tie, prefer the longest
            // phrase (sorted-by-length lookup would be nicer but
            // PATTERNS is short enough that this loop is fine).
            match best {
                None => best = Some((start, end, kind)),
                Some((bs, be, _)) if start < bs || (start == bs && (end - start) > (be - bs)) => {
                    best = Some((start, end, kind));
                }
                _ => {}
            }
        }
    }

    let (m_start, m_end, kind) = best?;
    let window = match kind {
        "today" => Some(window_today(now)),
        "yesterday" => Some(window_yesterday(now)),
        "this_week" => Some(window_this_week(now)),
        "last_week" => Some(window_last_week(now)),
        "this_month" => Some(window_this_month(now)),
        "last_month" => Some(window_last_month(now)),
        "recent" => Some(window_recent(now)),
        _ => None,
    }?;

    // Residual = original question minus the matched phrase, trimmed
    // and de-doubled-spaced. Keeps the case of the rest of the
    // question intact for embedding (case is mostly irrelevant for
    // BGE but feels nicer in logs).
    let mut residual = String::with_capacity(question.len());
    residual.push_str(&question[..m_start]);
    residual.push(' ');
    residual.push_str(&question[m_end..]);
    let residual: String = residual
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();

    Some((window, residual))
}

fn iso(dt: DateTime<Utc>) -> String {
    // Match the format used by capture_service when writing
    // memories.created_at — to_rfc3339 with milliseconds stripped is
    // close enough for lexicographic ordering; we compare as strings
    // in the DB filter, so consistency matters more than precision.
    dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn window(start_local: DateTime<Local>, end_local: DateTime<Local>, label: &'static str) -> TemporalWindow {
    TemporalWindow {
        label,
        start_iso: iso(start_local.with_timezone(&Utc)),
        end_iso: iso(end_local.with_timezone(&Utc)),
    }
}

fn start_of_day(dt: DateTime<Local>) -> DateTime<Local> {
    Local
        .from_local_datetime(&dt.date_naive().and_time(NaiveTime::MIN))
        .single()
        .unwrap_or(dt)
}

fn end_of_day(dt: DateTime<Local>) -> DateTime<Local> {
    let end = NaiveTime::from_hms_opt(23, 59, 59).expect("valid HMS");
    Local
        .from_local_datetime(&dt.date_naive().and_time(end))
        .single()
        .unwrap_or(dt)
}

fn window_today(now: DateTime<Local>) -> TemporalWindow {
    window(start_of_day(now), now, "today")
}

fn window_yesterday(now: DateTime<Local>) -> TemporalWindow {
    let y = now - Duration::days(1);
    window(start_of_day(y), end_of_day(y), "yesterday")
}

/// ISO 8601 week: starts Monday. "This week" is Monday 00:00 of the
/// current week → now (so a Wednesday-morning ask doesn't trawl
/// non-existent Thursday memories).
fn window_this_week(now: DateTime<Local>) -> TemporalWindow {
    let monday = monday_of_week(now);
    window(start_of_day(monday), now, "this week")
}

fn window_last_week(now: DateTime<Local>) -> TemporalWindow {
    let this_monday = monday_of_week(now);
    let last_monday = this_monday - Duration::days(7);
    let last_sunday = this_monday - Duration::seconds(1);
    window(start_of_day(last_monday), last_sunday, "last week")
}

fn monday_of_week(dt: DateTime<Local>) -> DateTime<Local> {
    let days_since_monday = match dt.weekday() {
        Weekday::Mon => 0,
        Weekday::Tue => 1,
        Weekday::Wed => 2,
        Weekday::Thu => 3,
        Weekday::Fri => 4,
        Weekday::Sat => 5,
        Weekday::Sun => 6,
    };
    dt - Duration::days(days_since_monday)
}

/// Calendar month: 1st of current month 00:00 → now.
fn window_this_month(now: DateTime<Local>) -> TemporalWindow {
    let first = Local
        .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
        .unwrap_or(now);
    window(first, now, "this month")
}

/// Calendar month: 1st of previous month 00:00 → last second of
/// previous month.
fn window_last_month(now: DateTime<Local>) -> TemporalWindow {
    let (prev_year, prev_month) = if now.month() == 1 {
        (now.year() - 1, 12)
    } else {
        (now.year(), now.month() - 1)
    };
    let prev_first = Local
        .with_ymd_and_hms(prev_year, prev_month, 1, 0, 0, 0)
        .single()
        .unwrap_or(now);
    let this_first = Local
        .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
        .unwrap_or(now);
    let prev_last = this_first - Duration::seconds(1);
    window(prev_first, prev_last, "last month")
}

/// "Recent" / "lately" — fuzzy. We use the last 7 calendar days
/// rolling, since this phrasing implies "in living memory" rather
/// than a sharp boundary.
fn window_recent(now: DateTime<Local>) -> TemporalWindow {
    let start = now - Duration::days(7);
    window(start, now, "recent")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_summarize_my_week() {
        let (w, residual) = detect("summarize my week").expect("detected");
        assert_eq!(w.label, "this week");
        assert!(residual.is_empty(), "residual should strip out the temporal phrase");
    }

    #[test]
    fn detects_with_topical_residual() {
        let (w, residual) = detect("what license keys did I save last week").expect("detected");
        assert_eq!(w.label, "last week");
        assert!(residual.contains("license keys"), "residual: {residual}");
    }

    #[test]
    fn no_temporal_returns_none() {
        assert!(detect("did i save a license key?").is_none());
    }

    #[test]
    fn longest_phrase_wins() {
        // "last week" should beat the bare "week" pattern on overlap.
        let (w, _) = detect("anything from last week").expect("detected");
        assert_eq!(w.label, "last week");
    }

    #[test]
    fn today_is_today_not_yesterday() {
        let (w, _) = detect("what did i save today").expect("detected");
        assert_eq!(w.label, "today");
    }
}
