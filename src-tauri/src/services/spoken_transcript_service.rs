use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};
use sysinfo::System;

use crate::{
    db::repositories::SharedMemoryRepository,
    errors::app_error::{AppError, AppResult},
    models::{AppContextSnapshot, Memory, MemoryInput, MemorySourceType},
    services::screenshot_store::SCREENSHOT_SOURCE_APP,
};

/// v0.5.18: this service owns the **Daily recap** memory — a single
/// per-local-day memory that rolls up everything captured that day:
/// spoken snippets, screenshots, bookmarks, clipboard notes, iPhone
/// captures. Pre-v0.5.18 it owned only the spoken-only "Daily
/// transcript" memory; the rename happens on next update.
///
/// The `external_id` prefix and source_app namespace stay as
/// `spoken-daily:` / `spoken` for backward-compat with users who
/// already have a Daily transcript memory from earlier versions —
/// renaming those would break Ask Recall retrieval against those
/// existing memories. Only the title and content body are reshaped.
const TRANSCRIPT_SOURCE_APP: &str = "spoken";
const TRANSCRIPT_EXTERNAL_ID_PREFIX: &str = "spoken-daily:";
/// Pre-v0.5.18 section marker. Still recognized on read so old
/// daily transcript memories parse cleanly; new writes use the
/// v0.5.18 `Spoken (N)` section header instead.
const TRANSCRIPT_SECTION_MARKER: &str = "\n\nTranscript\n\n";
/// v0.5.18 section marker prefix. The actual header is
/// `Spoken ({count})` so both readers and writers pin on the
/// `\n\nSpoken (` substring when extracting the spoken block.
const SPOKEN_SECTION_PREFIX: &str = "\n\nSpoken (";

/// Curated list of common transcription / dictation apps. Each entry is a
/// (process-substring, display-name) pair. The substring matches against the
/// running process name (lowercased) AND against the OS-reported frontmost
/// app/window. The first match wins, and its display-name is recorded in the
/// per-entry context label so the daily-transcript summary can list which
/// apps the user dictated through today.
///
/// Adding a new app: append a row. Substrings are matched case-insensitively
/// and must be unique enough to not false-match unrelated apps.
const TRANSCRIPTION_APPS: &[(&str, &str)] = &[
    ("spoken", "Spoken"),
    ("spokn", "Spokn"),
    ("macwhisper", "MacWhisper"),
    ("whisper memos", "Whisper Memos"),
    ("whispermemos", "Whisper Memos"),
    ("superwhisper", "SuperWhisper"),
    ("whisperflow", "WhisperFlow"),
    ("wisprflow", "Wispr Flow"),
    ("wispr flow", "Wispr Flow"),
    ("wispr", "Wispr"),
    ("aiko", "Aiko"),
    ("voiceink", "VoiceInk"),
    ("audiopen", "AudioPen"),
    ("voicenotes", "VoiceNotes"),
    ("voice notes", "VoiceNotes"),
    ("otter.ai", "Otter"),
    ("otter ai", "Otter"),
    ("otterhelper", "Otter"),
    ("granola", "Granola"),
    ("fathom", "Fathom"),
    ("descript", "Descript"),
    ("rev voice", "Rev Voice"),
    ("rev recorder", "Rev Voice"),
    ("krisp", "Krisp"),
    ("talkr", "Talkr"),
    ("speakai", "Speak"),
    ("speak.app", "Speak"),
    // macOS native dictation runs under coreaudio / SiriTTSD; the visible
    // frontmost label varies. Matched on common process names.
    ("dictationim", "macOS Dictation"),
    ("voicebanker", "macOS Dictation"),
    // Windows native speech recognition.
    ("windowsspeech", "Windows Speech"),
];

const TOPIC_STOPWORDS: &[&str] = &[
    "about", "after", "also", "and", "any", "app", "because", "been", "before", "being",
    "but", "could", "did", "does", "doing", "for", "from", "have", "into", "just", "know",
    "like", "look", "made", "make", "maybe", "need", "onto", "our", "out", "really", "said",
    "same", "should", "some", "something", "still", "than", "that", "their", "them", "then",
    "there", "these", "they", "this", "today", "transcript", "user", "using", "want", "what",
    "when", "which", "while", "with", "work", "would", "yeah", "your",
];

#[derive(Clone)]
pub struct SpokenTranscriptService {
    repository: SharedMemoryRepository,
}

impl SpokenTranscriptService {
    pub fn new(repository: SharedMemoryRepository) -> Self {
        Self { repository }
    }

    pub async fn capture_clipboard_snippet(
        &self,
        content: String,
        context: &AppContextSnapshot,
        detected_app: Option<&str>,
    ) -> AppResult<Memory> {
        let snippet = normalize_body_text(&content);
        if snippet.is_empty() {
            return Err(AppError::Invalid(
                "Transcript snippet cannot be empty.".into(),
            ));
        }

        let now_utc = Utc::now();
        let now_local = now_utc.with_timezone(&Local);
        let day_key = now_local.format("%Y-%m-%d").to_string();
        let external_id = format!("{TRANSCRIPT_EXTERNAL_ID_PREFIX}{day_key}");
        let title = format!("Daily recap · {}", now_local.format("%b %d"));
        // Prefer the detected transcription app name; fall back to whatever
        // the OS reported as frontmost (e.g. "Notes" — the destination), then
        // a generic "Transcription".
        let context_label = detected_app
            .map(str::to_string)
            .or_else(|| normalize_context_label(context.source_window.as_deref()))
            .or_else(|| normalize_context_label(context.source_app.as_deref()))
            .unwrap_or_else(|| "Transcription".to_string());
        let entry = build_entry_block(&now_local, &context_label, &snippet);

        // v0.5.18: query the day's other captures so the recap body
        // includes screenshots/bookmarks/notes alongside the spoken
        // section. This is the same query the post-save hook runs.
        let other_memories = self.list_day_memories(&now_local).await?;

        let date_label = now_local.format("%b %d").to_string();

        if let Some(existing) = self
            .repository
            .find_by_external_source(TRANSCRIPT_SOURCE_APP, &external_id)
            .await?
        {
            let prior_spoken_body = extract_spoken_body(&existing.content);
            let combined_body = append_entry_block(&prior_spoken_body, &entry);
            let content = compose_daily_recap_content(
                &title,
                &combined_body,
                &other_memories,
                existing.created_at.as_str(),
                now_utc.to_rfc3339().as_str(),
                &date_label,
            );

            return self
                .repository
                .update(
                    &existing.id,
                    MemoryInput {
                        source_type: Some(MemorySourceType::Manual),
                        title: Some(title),
                        content,
                        note: existing.note.clone(),
                        project_id: existing.project_id.clone(),
                        url: None,
                        external_id: Some(external_id),
                        folder_path: None,
                        source_app: Some(TRANSCRIPT_SOURCE_APP.to_string()),
                        source_window: Some(context_label),
                        created_at: Some(existing.created_at),
                        updated_at: Some(now_utc.to_rfc3339()),
                    },
                )
                .await;
        }

        let content = compose_daily_recap_content(
            &title,
            &entry,
            &other_memories,
            now_utc.to_rfc3339().as_str(),
            now_utc.to_rfc3339().as_str(),
            &date_label,
        );

        self.repository
            .create(MemoryInput {
                source_type: Some(MemorySourceType::Manual),
                title: Some(title),
                content,
                note: None,
                project_id: None,
                url: None,
                external_id: Some(external_id),
                folder_path: None,
                source_app: Some(TRANSCRIPT_SOURCE_APP.to_string()),
                source_window: Some(context_label),
                created_at: Some(now_utc.to_rfc3339()),
                updated_at: Some(now_utc.to_rfc3339()),
            })
            .await
    }

    /// v0.5.18: rebuild today's Daily recap from the day's current
    /// captures. Called from the capture pipeline's post-save hook
    /// after a non-spoken memory lands (screenshot, bookmark, note,
    /// iPhone import, etc.) so the recap stays in sync without the
    /// caller having to reach into this service's internals.
    ///
    /// Idempotent: rebuilds the body purely from current DB state.
    /// The spoken section (the only data not stored as separate
    /// memory rows) is preserved verbatim from the existing recap
    /// memory if one exists.
    ///
    /// Returns `Ok(None)` when there's nothing to recap yet (no
    /// captures of any kind for today AND no existing recap memory).
    /// Returns `Ok(Some(memory))` after a successful create/update.
    pub async fn rebuild_recap_for_today(&self) -> AppResult<Option<Memory>> {
        let now_utc = Utc::now();
        let now_local = now_utc.with_timezone(&Local);
        let day_key = now_local.format("%Y-%m-%d").to_string();
        let external_id = format!("{TRANSCRIPT_EXTERNAL_ID_PREFIX}{day_key}");
        let title = format!("Daily recap · {}", now_local.format("%b %d"));
        let date_label = now_local.format("%b %d").to_string();

        let other_memories = self.list_day_memories(&now_local).await?;

        if let Some(existing) = self
            .repository
            .find_by_external_source(TRANSCRIPT_SOURCE_APP, &external_id)
            .await?
        {
            let spoken_body = extract_spoken_body(&existing.content);
            let content = compose_daily_recap_content(
                &title,
                &spoken_body,
                &other_memories,
                existing.created_at.as_str(),
                now_utc.to_rfc3339().as_str(),
                &date_label,
            );
            // Skip the write if nothing changed. Avoids tickling
            // updated_at and re-firing AI summary regeneration on
            // every screenshot save.
            if content == existing.content {
                return Ok(Some(existing));
            }
            return self
                .repository
                .update(
                    &existing.id,
                    MemoryInput {
                        source_type: Some(MemorySourceType::Manual),
                        title: Some(title),
                        content,
                        note: existing.note.clone(),
                        project_id: existing.project_id.clone(),
                        url: None,
                        external_id: Some(external_id),
                        folder_path: None,
                        source_app: Some(TRANSCRIPT_SOURCE_APP.to_string()),
                        source_window: existing.source_window.clone(),
                        created_at: Some(existing.created_at),
                        updated_at: Some(now_utc.to_rfc3339()),
                    },
                )
                .await
                .map(Some);
        }

        // No existing recap. Only create one if there's something to
        // include — otherwise the post-save hook would generate empty
        // recaps for days where nothing was captured (impossible by
        // construction since the hook fires on save, but keeps the
        // code defensive).
        if other_memories.is_empty() {
            return Ok(None);
        }
        let content = compose_daily_recap_content(
            &title,
            "",
            &other_memories,
            now_utc.to_rfc3339().as_str(),
            now_utc.to_rfc3339().as_str(),
            &date_label,
        );
        self.repository
            .create(MemoryInput {
                source_type: Some(MemorySourceType::Manual),
                title: Some(title),
                content,
                note: None,
                project_id: None,
                url: None,
                external_id: Some(external_id),
                folder_path: None,
                source_app: Some(TRANSCRIPT_SOURCE_APP.to_string()),
                source_window: None,
                created_at: Some(now_utc.to_rfc3339()),
                updated_at: Some(now_utc.to_rfc3339()),
            })
            .await
            .map(Some)
    }

    async fn list_day_memories(
        &self,
        now_local: &DateTime<Local>,
    ) -> AppResult<Vec<Memory>> {
        let day_start_local = local_day_start(now_local.date_naive());
        let day_end_local = day_start_local + chrono::Duration::days(1);
        let start_utc = day_start_local.with_timezone(&Utc).to_rfc3339();
        let end_utc = day_end_local.with_timezone(&Utc).to_rfc3339();
        self.repository
            .list_memories_for_day(&start_utc, &end_utc)
            .await
    }
}

/// v0.5.18: take a NaiveDate (local) and return the local-tz
/// midnight of that day as a DateTime<Local>. Handles ambiguous
/// hours during DST transitions by falling back to the earliest
/// valid moment for the date.
fn local_day_start(date: NaiveDate) -> DateTime<Local> {
    let naive = date
        .and_hms_opt(0, 0, 0)
        .expect("hms 0,0,0 always valid");
    Local
        .from_local_datetime(&naive)
        .earliest()
        .unwrap_or_else(|| {
            // Spring-forward edge case (rare): the wall clock skips
            // 00:00. Fall back to UTC midnight of that date — the
            // boundary is approximate but the recap query is
            // chronological-by-created_at, not exact-day-boundary.
            Utc.from_utc_datetime(&naive).with_timezone(&Local)
        })
}

/// Detects whether a known transcription/dictation app shows up in the
/// frontmost-app snapshot. Returns the canonical display name if one matches.
///
/// Most transcription apps paste their output into the *destination* app
/// (Notes, Chrome, Slack…), so the OS frontmost is rarely the transcription
/// app itself — `detect_running_transcription_app` is the more reliable
/// signal. This is kept as a secondary hint for apps that briefly pop a
/// floating window during dictation.
pub fn detect_transcription_context_app(context: &AppContextSnapshot) -> Option<&'static str> {
    let candidates = [context.source_app.as_deref(), context.source_window.as_deref()]
        .into_iter()
        .flatten()
        .map(|value| value.to_ascii_lowercase())
        .collect::<Vec<_>>();

    for (substring, display) in TRANSCRIPTION_APPS {
        if candidates.iter().any(|haystack| haystack.contains(*substring)) {
            return Some(*display);
        }
    }
    None
}

/// Detects whether any of our curated transcription apps are currently
/// running on this machine. Returns the canonical display name of the first
/// match, or None if no transcription app is running.
///
/// This is what actually fires reliably during a dictation session because
/// transcription apps run in the background while the user is in their
/// destination app. The result is cached for ~1.5s to avoid scanning the
/// process table on every clipboard tick (clipboard polls every ~900ms).
pub fn detect_running_transcription_app() -> Option<&'static str> {
    static CACHE: Mutex<Option<(Instant, Option<&'static str>)>> = Mutex::new(None);
    const TTL: Duration = Duration::from_millis(1500);

    if let Ok(guard) = CACHE.lock() {
        if let Some((stamped_at, value)) = *guard {
            if stamped_at.elapsed() < TTL {
                return value;
            }
        }
    }

    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let process_names: Vec<String> = system
        .processes()
        .values()
        .map(|process| process.name().to_string_lossy().to_ascii_lowercase())
        .collect();

    let mut detected: Option<&'static str> = None;
    for (substring, display) in TRANSCRIPTION_APPS {
        if process_names.iter().any(|name| name.contains(*substring)) {
            detected = Some(*display);
            break;
        }
    }

    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some((Instant::now(), detected));
    }

    detected
}

/// Convenience: any transcription app currently running OR in the frontmost
/// snapshot? Returns the canonical display name to record on the daily entry.
pub fn detect_transcription_app(context: &AppContextSnapshot) -> Option<&'static str> {
    detect_running_transcription_app().or_else(|| detect_transcription_context_app(context))
}

/// Classifier: does this clipboard content look like spoken-language text we
/// should fold into the daily transcript? Returns false for URLs, file paths,
/// code-shaped content, tabular data, very long content — those should remain
/// independent memories so the bookmark intelligence + link enrichment paths
/// still get a chance at them.
pub fn looks_like_transcript_text(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() || trimmed.len() > 4000 {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();

    // URLs and protocol prefixes — these belong to link enrichment.
    if ["http://", "https://", "www.", "ftp://", "file://"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        return false;
    }

    // File-path shapes (POSIX absolute, home-relative, multiple backslashes).
    if trimmed.starts_with('/')
        || trimmed.starts_with("~/")
        || trimmed.matches('\\').count() >= 2
    {
        return false;
    }
    // Windows drive paths like `C:\…`.
    if trimmed.len() >= 3 {
        let bytes = trimmed.as_bytes();
        if bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\' {
            return false;
        }
    }

    // Code fences or 3+ consecutive lines that look like code structure.
    if trimmed.contains("```") {
        return false;
    }
    let mut code_run = 0usize;
    for line in trimmed.lines() {
        let leading = line.trim_start();
        let starts_code_glyph = leading.starts_with('{')
            || leading.starts_with('}')
            || leading.starts_with('(')
            || leading.starts_with('<')
            || leading.starts_with("//")
            || leading.starts_with("/*")
            || leading.starts_with("import ")
            || leading.starts_with("function ")
            || leading.starts_with("const ")
            || leading.starts_with("def ");
        if starts_code_glyph {
            code_run += 1;
            if code_run >= 3 {
                return false;
            }
        } else {
            code_run = 0;
        }
    }

    // Tabular paste — multiple tabs strongly suggests spreadsheet content.
    if trimmed.matches('\t').count() > 1 {
        return false;
    }

    // Spoken text always has at least one space (rules out single-token IDs).
    if !trimmed.contains(' ') {
        return false;
    }

    // Letter-to-other-character ratio — natural speech is letter-heavy.
    let letters = trimmed.chars().filter(|c| c.is_alphabetic()).count();
    let other = trimmed
        .chars()
        .filter(|c| !c.is_alphabetic() && !c.is_whitespace())
        .count();
    if other > 0 && letters / other.max(1) < 4 {
        return false;
    }

    true
}

fn normalize_newlines(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

fn normalize_body_text(value: &str) -> String {
    let mut output = Vec::new();
    let mut blank_count = 0usize;

    for line in normalize_newlines(value).lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            blank_count += 1;
            if blank_count <= 1 && !output.is_empty() {
                output.push(String::new());
            }
        } else {
            blank_count = 0;
            output.push(trimmed.to_string());
        }
    }

    output.join("\n").trim().to_string()
}

fn normalize_context_label(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn build_entry_block(timestamp: &DateTime<Local>, context_label: &str, snippet: &str) -> String {
    format!(
        "[{} · {}]\n{}",
        timestamp.format("%-I:%M %p"),
        context_label,
        snippet
    )
}

fn append_entry_block(existing_body: &str, next_entry: &str) -> String {
    let trimmed = existing_body.trim();
    if trimmed.is_empty() {
        next_entry.to_string()
    } else {
        format!("{trimmed}\n\n{next_entry}")
    }
}

/// Extract the spoken-transcript body from the content of a recap
/// memory. Recognizes both the v0.5.18 `Spoken (N)\n\n` section
/// header and the legacy `Transcript\n\n` marker so existing
/// memories survive the upgrade without losing their snippets.
/// Returns the section body (just the entries, no header) trimmed.
/// Returns an EMPTY string when neither marker is present — this is
/// the common case for a recap memory that was created by a
/// non-spoken capture first (e.g. the day started with a screenshot,
/// no spoken snippets yet). Returning the whole content would cause
/// the next spoken snippet to get appended on top of an unrelated
/// Summary/Screenshots block, corrupting the body.
fn extract_spoken_body(content: &str) -> String {
    // v0.5.18 marker first; falls through to v0.1.x marker.
    if let Some((_, after_header)) = content.split_once(SPOKEN_SECTION_PREFIX) {
        // Header is `Spoken (N)\n\n<body>` — strip up to first
        // `\n\n` to drop the count line.
        if let Some((_, body)) = after_header.split_once("\n\n") {
            // Stop at the next section header (a bare `<Label> (N)`
            // line preceded by `\n\n`). Anything after is a
            // different section; only the spoken snippets belong
            // here.
            let trimmed_body = match find_next_section_start(body) {
                Some(idx) => &body[..idx],
                None => body,
            };
            return trimmed_body.trim().to_string();
        }
    }
    if let Some((_, body)) = content.split_once(TRANSCRIPT_SECTION_MARKER) {
        return body.trim().to_string();
    }
    // No spoken section yet — caller appends to an empty body.
    String::new()
}

/// Pre-v0.5.18 alias kept so call sites and the test module stay
/// terse. Identical to `extract_spoken_body`.
fn extract_transcript_body(content: &str) -> String {
    extract_spoken_body(content)
}

/// Find the start of the next `<Label> (count)` section header
/// inside the spoken-block tail. Returns the byte index of the
/// preceding `\n\n` (so the spoken block ends cleanly without
/// trailing whitespace). Used to bound `extract_spoken_body` when
/// the recap has Screenshots/Bookmarks/etc. sections after Spoken.
fn find_next_section_start(text: &str) -> Option<usize> {
    let mut search_from = 0usize;
    while let Some(idx) = text[search_from..].find("\n\n") {
        let absolute = search_from + idx;
        let after = &text[absolute + 2..];
        if let Some(line_end) = after.find('\n') {
            let header = &after[..line_end];
            if is_section_header(header) {
                return Some(absolute);
            }
        } else if is_section_header(after) {
            return Some(absolute);
        }
        search_from = absolute + 2;
    }
    None
}

/// True when `line` looks like a section header — `<Label> (count)`
/// with Label being one of our known sections. Strict on label set
/// to avoid false-positive matches inside transcript content.
fn is_section_header(line: &str) -> bool {
    let labels = [
        "Spoken",
        "Screenshots",
        "Bookmarks",
        "From iPhone",
        "Saved notes",
    ];
    labels.iter().any(|label| {
        line.starts_with(label)
            && line[label.len()..].trim_start().starts_with('(')
            && line.trim_end().ends_with(')')
    })
}

/// v0.5.18: compose the full Daily recap memory body. Sections:
///
///   ```
///   {title}
///
///   Summary
///   - 10 spoken snippets, 4 screenshots, 2 bookmarks today.
///   - Active from 8:14 AM to 9:42 PM.
///
///   Spoken (10)
///   [9:12 AM · Spoken]
///   …entries…
///
///   Screenshots (4)
///   - 9:14 AM · "Acme pricing tiers" — preview…
///   …
///   ```
///
/// Sections are emitted only when they have content; a day with
/// no spoken snippets and 3 bookmarks shows just the Summary +
/// Bookmarks blocks.
fn compose_daily_recap_content(
    title: &str,
    spoken_body: &str,
    other_memories: &[Memory],
    first_captured_at: &str,
    last_captured_at: &str,
    date_label: &str,
) -> String {
    let _ = date_label;
    let summary = build_recap_summary(
        spoken_body,
        other_memories,
        first_captured_at,
        last_captured_at,
    );

    let mut sections: Vec<String> = Vec::new();
    sections.push(format!("Summary\n{summary}"));

    let spoken_count = count_spoken_entries(spoken_body);
    if spoken_count > 0 {
        sections.push(format!(
            "Spoken ({spoken_count})\n\n{}",
            spoken_body.trim()
        ));
    }

    let groups = group_other_memories(other_memories);
    for (label, items) in groups {
        if items.is_empty() {
            continue;
        }
        let lines: Vec<String> = items
            .iter()
            .map(|memory| format_other_memory_line(memory))
            .collect();
        sections.push(format!("{label} ({})\n{}", items.len(), lines.join("\n")));
    }

    format!("{title}\n\n{}", sections.join("\n\n"))
}

/// Rule-based summary used as a fallback when the LLM summary
/// hasn't been generated yet (or the LLM is unavailable). Reads
/// section counts + active span; no content-aware claims.
fn build_recap_summary(
    spoken_body: &str,
    other_memories: &[Memory],
    first_captured_at: &str,
    last_captured_at: &str,
) -> String {
    let spoken_count = count_spoken_entries(spoken_body);
    let groups = group_other_memories(other_memories);

    let mut count_phrases: Vec<String> = Vec::new();
    if spoken_count > 0 {
        count_phrases.push(format!(
            "{spoken_count} spoken snippet{}",
            if spoken_count == 1 { "" } else { "s" }
        ));
    }
    for (label, items) in &groups {
        if items.is_empty() {
            continue;
        }
        let n = items.len();
        let phrase = match *label {
            "Screenshots" => format!("{n} screenshot{}", if n == 1 { "" } else { "s" }),
            "Bookmarks" => format!("{n} bookmark{}", if n == 1 { "" } else { "s" }),
            "From iPhone" => format!("{n} from iPhone"),
            "Saved notes" => format!("{n} saved note{}", if n == 1 { "" } else { "s" }),
            other => format!("{n} {other}"),
        };
        count_phrases.push(phrase);
    }

    let count_line = if count_phrases.is_empty() {
        "- Nothing captured yet.".to_string()
    } else {
        format!("- {}.", join_with_oxford_and(&count_phrases))
    };

    let first_local = parse_rfc3339_to_local(first_captured_at);
    let last_local = parse_rfc3339_to_local(last_captured_at);
    let active_span = match (first_local, last_local) {
        (Some(first), Some(last)) => format!(
            "- Active from {} to {}.",
            first.format("%-I:%M %p"),
            last.format("%-I:%M %p")
        ),
        _ => "- Active throughout the day.".to_string(),
    };

    let mut lines = vec![count_line, active_span];

    let app_summary = extract_context_labels(spoken_body);
    if !app_summary.is_empty() {
        lines.push(format!("- Spoken via {}.", app_summary.join(", ")));
    }

    let topics = extract_top_topics(spoken_body, 4);
    if !topics.is_empty() {
        lines.push(format!("- Mentioned often: {}.", topics.join(", ")));
    }

    lines.join("\n")
}

/// Count `[time · app]` blocks in the spoken body. The blocks are
/// the unit by which `build_entry_block` writes snippets, so this
/// matches the actual snippet count.
fn count_spoken_entries(body: &str) -> usize {
    body.lines()
        .filter(|line| line.starts_with('[') && line.ends_with(']'))
        .count()
}

/// Categorize the day's non-spoken memories into sections. Order
/// is fixed so the recap reads consistently day to day:
/// Screenshots → Bookmarks → From iPhone → Saved notes.
fn group_other_memories(memories: &[Memory]) -> Vec<(&'static str, Vec<&Memory>)> {
    let mut screenshots: Vec<&Memory> = Vec::new();
    let mut bookmarks: Vec<&Memory> = Vec::new();
    let mut from_iphone: Vec<&Memory> = Vec::new();
    let mut saved_notes: Vec<&Memory> = Vec::new();
    for memory in memories {
        if memory.source_type == MemorySourceType::Bookmark {
            bookmarks.push(memory);
            continue;
        }
        match memory.source_app.as_deref() {
            Some(app) if app == SCREENSHOT_SOURCE_APP => screenshots.push(memory),
            Some("mobile") => from_iphone.push(memory),
            _ => saved_notes.push(memory),
        }
    }
    vec![
        ("Screenshots", screenshots),
        ("Bookmarks", bookmarks),
        ("From iPhone", from_iphone),
        ("Saved notes", saved_notes),
    ]
}

/// Render one non-spoken memory as a recap bullet:
///   `- 9:14 AM · "Acme pricing tiers" — preview…`
/// Bookmarks get the domain in parens after the title; everything
/// else uses content/summary_text/preview_text as the preview.
fn format_other_memory_line(memory: &Memory) -> String {
    let local_time = parse_rfc3339_to_local(&memory.created_at)
        .map(|dt| dt.format("%-I:%M %p").to_string())
        .unwrap_or_else(|| "—".to_string());
    let title = display_title_for_memory(memory);
    let domain_suffix = if memory.source_type == MemorySourceType::Bookmark {
        memory
            .resolved_domain
            .as_deref()
            .or(memory.domain.as_deref())
            .map(|d| format!(" ({d})"))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let preview = preview_for_memory(memory, 110);
    if preview.is_empty() {
        format!("- {local_time} · \"{title}\"{domain_suffix}")
    } else {
        format!("- {local_time} · \"{title}\"{domain_suffix} — {preview}")
    }
}

/// Pick a displayable title for a memory. Prefers explicit title,
/// then resolved_title (link enrichment), then the first line of
/// content, then a generic fallback.
fn display_title_for_memory(memory: &Memory) -> String {
    if let Some(value) = memory
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return value.to_string();
    }
    if let Some(value) = memory
        .resolved_title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return value.to_string();
    }
    let first_line = memory
        .content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    if first_line.is_empty() {
        "Untitled".to_string()
    } else {
        truncate_chars(first_line, 60)
    }
}

/// Pick a short preview for a memory's bullet. Prefers the
/// pre-computed `summary_text` (cheap, already cleaned), then
/// `preview_text`, then content. Truncated to `max_chars` chars
/// with ellipsis.
fn preview_for_memory(memory: &Memory, max_chars: usize) -> String {
    let candidate = memory
        .summary_text
        .as_deref()
        .or(memory.preview_text.as_deref())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| {
            memory
                .content
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or("")
                .to_string()
        });
    if candidate.is_empty() {
        return String::new();
    }
    truncate_chars(&candidate, max_chars)
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let total = text.chars().count();
    if total <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{}…", truncated.trim_end())
}

/// Join phrases with Oxford-comma "and": `["a", "b", "c"]` →
/// `"a, b, and c"`. Single phrase returned as-is; two phrases
/// joined with " and " (no comma).
fn join_with_oxford_and(phrases: &[String]) -> String {
    match phrases.len() {
        0 => String::new(),
        1 => phrases[0].clone(),
        2 => format!("{} and {}", phrases[0], phrases[1]),
        _ => {
            let head = phrases[..phrases.len() - 1].join(", ");
            format!("{}, and {}", head, phrases[phrases.len() - 1])
        }
    }
}

fn parse_rfc3339_to_local(value: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|datetime| datetime.with_timezone(&Local))
}

fn extract_context_labels(body: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();

    for line in body.lines() {
        if !line.starts_with('[') || !line.ends_with(']') {
            continue;
        }

        let inner = line.trim_matches(['[', ']']);
        let mut parts = inner.split(" · ");
        let _ = parts.next();
        if let Some(label) = parts.next() {
            let label = label.trim();
            if !label.is_empty() {
                seen.insert(label.to_string());
            }
        }
    }

    seen.into_iter().take(4).collect()
}

fn extract_top_topics(body: &str, limit: usize) -> Vec<String> {
    let stopwords = TOPIC_STOPWORDS.iter().copied().collect::<HashSet<_>>();
    let mut scores = HashMap::<String, usize>::new();

    for line in body.lines() {
        if line.starts_with('[') && line.ends_with(']') {
            continue;
        }

        for raw_token in line
            .split(|character: char| !character.is_alphanumeric() && character != '-' && character != '_')
            .map(|token| token.trim_matches(['-', '_']))
            .filter(|token| token.len() >= 4)
        {
            let normalized = raw_token.to_ascii_lowercase();
            if stopwords.contains(normalized.as_str())
                || normalized.chars().all(|character| character.is_numeric())
            {
                continue;
            }

            *scores.entry(normalized).or_insert(0) += 1;
        }
    }

    let mut ranked = scores.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));

    ranked
        .into_iter()
        .filter(|(_, count)| *count >= 2)
        .take(limit)
        .map(|(token, _)| token)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{str::FromStr, sync::Arc};

    use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};

    use crate::{
        db::{migrations::run_migrations, sqlite_memory_repository::SqliteMemoryRepository},
        models::AppContextSnapshot,
    };

    use super::{
        compose_daily_recap_content, detect_transcription_context_app, extract_transcript_body,
        SpokenTranscriptService,
    };

    async fn make_service() -> SpokenTranscriptService {
        let options = SqliteConnectOptions::from_str(":memory:").expect("in-memory options");
        let pool = SqlitePool::connect_with(options).await.expect("pool");
        run_migrations(&pool).await.expect("migrations");
        SpokenTranscriptService::new(Arc::new(SqliteMemoryRepository::new(pool)))
    }

    #[test]
    fn detects_known_transcription_apps_in_context() {
        // Spoken still detects.
        assert_eq!(
            detect_transcription_context_app(&AppContextSnapshot {
                source_app: Some("Spoken".into()),
                source_window: None,
            }),
            Some("Spoken"),
        );
        assert_eq!(
            detect_transcription_context_app(&AppContextSnapshot {
                source_app: Some("Chrome".into()),
                source_window: Some("Spoken Overlay".into()),
            }),
            Some("Spoken"),
        );
        // Other transcription apps detect too.
        assert_eq!(
            detect_transcription_context_app(&AppContextSnapshot {
                source_app: Some("MacWhisper".into()),
                source_window: None,
            }),
            Some("MacWhisper"),
        );
        assert_eq!(
            detect_transcription_context_app(&AppContextSnapshot {
                source_app: Some("Wispr Flow".into()),
                source_window: None,
            }),
            Some("Wispr Flow"),
        );
        // Unrelated apps return None.
        assert_eq!(
            detect_transcription_context_app(&AppContextSnapshot {
                source_app: Some("Chrome".into()),
                source_window: Some("ChatGPT".into()),
            }),
            None,
        );
    }

    #[tokio::test]
    async fn spoken_snippets_roll_into_one_daily_memory() {
        let service = make_service().await;
        let context = AppContextSnapshot {
            source_app: Some("Spoken".into()),
            source_window: Some("Spoken Overlay".into()),
        };

        let first = service
            .capture_clipboard_snippet(
                "We should tighten the pricing page copy.".into(),
                &context,
                Some("Spoken"),
            )
            .await
            .expect("first transcript save");
        let second = service
            .capture_clipboard_snippet(
                "Let's also revisit onboarding tomorrow.".into(),
                &context,
                Some("Spoken"),
            )
            .await
            .expect("second transcript save");

        assert_eq!(first.id, second.id);
        assert_eq!(second.source_app.as_deref(), Some("spoken"));
        assert!(second.content.starts_with("Daily recap · "));
        assert!(second.content.contains("Summary"));
        // v0.5.18: spoken section header is `Spoken (N)` instead
        // of the legacy `Transcript`. The extractor still reads
        // legacy memories — verified separately below.
        assert!(second.content.contains("Spoken (2)"));
        let body = extract_transcript_body(&second.content);
        assert!(body.contains("pricing page copy"));
        assert!(body.contains("revisit onboarding tomorrow"));
    }

    #[test]
    fn extract_spoken_body_handles_legacy_transcript_marker() {
        // Pre-v0.5.18 daily-transcript memories used the
        // `\n\nTranscript\n\n` marker. Existing rows need to
        // round-trip through extract_transcript_body without
        // losing their snippets after the upgrade.
        let legacy = "Daily transcript · Apr 25\n\nSummary\n- 2 spoken \
                      snippets captured today.\n\nTranscript\n\n[9:12 AM · \
                      Spoken]\nFirst snippet\n\n[10:04 AM · Spoken]\nSecond \
                      snippet";
        let body = extract_transcript_body(legacy);
        assert!(body.contains("First snippet"));
        assert!(body.contains("Second snippet"));
        assert!(!body.contains("Daily transcript"));
        assert!(!body.contains("Summary"));
    }

    #[test]
    fn recap_document_contains_summary_and_spoken_sections() {
        let content = compose_daily_recap_content(
            "Daily recap · Apr 25",
            "[9:12 AM · Spoken]\nTest snippet",
            &[],
            "2026-04-25T03:42:00Z",
            "2026-04-25T04:12:00Z",
            "Apr 25",
        );

        assert!(content.starts_with("Daily recap · Apr 25"));
        assert!(content.contains("Summary"));
        assert!(content.contains("Spoken (1)"));
        assert!(content.contains("Test snippet"));
        assert!(content.contains("1 spoken snippet"));
    }

    #[test]
    fn recap_document_emits_zero_sections_when_empty() {
        let content = compose_daily_recap_content(
            "Daily recap · Apr 25",
            "",
            &[],
            "2026-04-25T03:42:00Z",
            "2026-04-25T04:12:00Z",
            "Apr 25",
        );

        assert!(content.starts_with("Daily recap · Apr 25"));
        assert!(content.contains("Summary"));
        assert!(content.contains("Nothing captured yet"));
        assert!(!content.contains("Spoken ("));
        assert!(!content.contains("Screenshots ("));
    }
}
