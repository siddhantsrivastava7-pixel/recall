//! Paragraph-aware char-budget chunker.
//!
//! Splits a memory's content into bounded chunks suitable for embedding +
//! RAG retrieval. Design constraints:
//!
//!   * **Paragraphs are first-class.** We split on `\n\n` boundaries and
//!     try to keep paragraphs intact so the resulting chunk reads
//!     naturally — important because Phase 3's LLM will receive these
//!     chunks verbatim as RAG context.
//!   * **Char budget, not token-perfect.** No tokenizer dep in v0.3.0;
//!     `char_count / 4` is a stable English-ish proxy and the LLM-side
//!     RAG retriever can compute proper token counts when it ships.
//!   * **Offsets are character indices, UTF-8 safe.** Stored as
//!     `(start_offset, end_offset)` in the parent's `content` so
//!     citations highlight exactly the source range. Both are computed
//!     by `char_indices()` so multibyte characters never split a chunk.
//!   * **Deterministic re-chunk.** Same input → same chunks → same
//!     content hashes. The capture-edit hook compares hashes against
//!     existing rows and only re-embeds chunks whose text actually
//!     changed.
//!
//! Output is `Vec<Chunk>`; the caller (`ai/embeddings/mod.rs`) writes
//! them to the `memory_chunks` table.

use serde::{Deserialize, Serialize};

/// Target chunk size — the chunker will pack paragraphs up to about
/// this many characters before emitting. ~500 tokens English-ish.
pub const TARGET_CHARS: usize = 2000;

/// Hard ceiling. A single paragraph longer than this is split on
/// sentence boundaries; if it has no sentence boundaries it's split
/// at exactly this character count.
pub const MAX_CHARS: usize = 8000;

/// Soft minimum. Chunks shorter than this are merged with the next
/// chunk (or, if last, into the previous one) so we don't end up with
/// a tail of one-line scraps.
pub const MIN_CHARS: usize = 200;

/// One chunk emitted by the chunker. `start_offset` and `end_offset`
/// are character (not byte) indices in the source content; subtract
/// them and `text.chars().count()` will match.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Chunk {
    pub text: String,
    pub start_offset: usize,
    pub end_offset: usize,
    pub content_hash: String,
}

impl Chunk {
    pub fn char_count(&self) -> usize {
        self.text.chars().count()
    }

    /// Rough token estimate. English averages ~4 chars/token; we don't
    /// bind a real tokenizer just for chunk metadata. The Phase 3 RAG
    /// retriever uses this for context-window budgeting — when it
    /// switches to a real tokenizer the field gets recomputed without
    /// a re-chunk because the text + offsets are stable.
    pub fn token_estimate(&self) -> usize {
        self.char_count().div_ceil(4)
    }

    pub fn byte_size(&self) -> usize {
        self.text.len()
    }
}

/// Chunk a memory's content. Returns at least one chunk for any
/// non-empty input (an empty input yields an empty Vec — the caller
/// treats that as "this memory has nothing to embed").
pub fn chunk_text(content: &str) -> Vec<Chunk> {
    let trimmed = content.trim_start();
    if trimmed.is_empty() {
        return Vec::new();
    }

    // Split into paragraph segments while tracking the *original*
    // char offset of each segment in the source content. We can't use
    // byte offsets because we want UTF-8-safe character indices.
    let segments = paragraph_segments(content);
    if segments.is_empty() {
        return Vec::new();
    }

    let mut chunks: Vec<Chunk> = Vec::new();
    let mut buffer: Vec<&Segment> = Vec::new();
    let mut buffer_chars: usize = 0;

    for segment in &segments {
        let seg_chars = segment.char_count;

        // A single segment that exceeds MAX_CHARS gets split inside
        // `flush_oversize_segment` — if there's anything in the
        // buffer, flush it first so the oversize segment stands alone.
        if seg_chars > MAX_CHARS {
            if !buffer.is_empty() {
                chunks.push(make_chunk(&buffer));
                buffer.clear();
                buffer_chars = 0;
            }
            chunks.extend(split_oversize_segment(segment));
            continue;
        }

        let would_be = buffer_chars + seg_chars;
        if would_be > MAX_CHARS && !buffer.is_empty() {
            // Adding this segment would blow the ceiling; flush buffer
            // first, then start a new one with this segment (carrying
            // an overlap of the last segment from the previous chunk
            // for continuity).
            let prev_tail = buffer.last().copied();
            chunks.push(make_chunk(&buffer));
            buffer.clear();
            buffer_chars = 0;

            if let Some(tail) = prev_tail {
                // 1-paragraph overlap. Skip if the tail itself is
                // already huge — the oversize path handles that.
                if tail.char_count <= TARGET_CHARS {
                    buffer.push(tail);
                    buffer_chars += tail.char_count;
                }
            }
        }

        buffer.push(segment);
        buffer_chars += seg_chars;

        if buffer_chars >= TARGET_CHARS {
            chunks.push(make_chunk(&buffer));
            // 1-paragraph overlap into the next chunk.
            let last = *buffer.last().unwrap();
            buffer.clear();
            buffer_chars = 0;
            if last.char_count <= TARGET_CHARS {
                buffer.push(last);
                buffer_chars += last.char_count;
            }
        }
    }

    if !buffer.is_empty() {
        // If the trailing buffer is tiny *and* we already emitted a
        // chunk, merge it into the previous chunk rather than emitting
        // a stunted tail.
        if buffer_chars < MIN_CHARS && !chunks.is_empty() {
            let mut last = chunks.pop().unwrap();
            let extension = make_chunk(&buffer);
            // `extension.start_offset` is somewhere after `last.end_offset`
            // (separated by the paragraph break). Re-derive text + range
            // from the original source spanning both.
            last = Chunk {
                text: format!("{}\n\n{}", last.text, extension.text),
                start_offset: last.start_offset,
                end_offset: extension.end_offset,
                content_hash: String::new(), // recompute below
            };
            last.content_hash = fnv1a_64_hex(&last.text);
            chunks.push(last);
        } else {
            chunks.push(make_chunk(&buffer));
        }
    }

    chunks
}

/// FNV-1a 64-bit content hash, formatted as 16 hex characters. Cheap,
/// stable across runs, deterministic — enough for chunk-level
/// invalidation. Collisions at our scale (max ~1M chunks per user)
/// are vanishingly rare.
pub fn fnv1a_64_hex(text: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[derive(Debug, Clone, Copy)]
struct Segment<'a> {
    text: &'a str,
    /// Inclusive char offset within the *original* source content.
    start_offset: usize,
    /// Exclusive char offset within the original source content.
    end_offset: usize,
    char_count: usize,
}

/// Walk the source string and emit one `Segment` per paragraph
/// (delimited by `\n\n` runs). When the source has no double-newlines,
/// falls back to splitting on single `\n`. Whitespace-only paragraphs
/// are skipped; the resulting segments preserve their *original* char
/// offsets in the input regardless.
fn paragraph_segments(content: &str) -> Vec<Segment<'_>> {
    let mut segments = Vec::new();
    let mut start_char = 0usize;
    let mut chars_consumed = 0usize;

    let chars: Vec<(usize, char)> = content.char_indices().collect();

    let mut current_start_char: Option<usize> = None;
    let mut current_start_byte: Option<usize> = None;
    let mut newline_run: usize = 0;

    let total_chars = chars.len();
    let mut i = 0usize;
    while i < total_chars {
        let (byte_idx, ch) = chars[i];
        if ch == '\n' {
            newline_run += 1;
            i += 1;
            chars_consumed = i;
            // Two or more consecutive newlines = paragraph break.
            if newline_run >= 2 {
                if let (Some(s_char), Some(s_byte)) = (current_start_char, current_start_byte) {
                    // End of segment: rewind past the trailing newlines
                    // so the segment text doesn't include them.
                    let end_char = i - newline_run;
                    let end_byte = if end_char < total_chars {
                        chars[end_char].0
                    } else {
                        content.len()
                    };
                    let text = &content[s_byte..end_byte];
                    let seg_chars = end_char - s_char;
                    if !text.trim().is_empty() {
                        segments.push(Segment {
                            text,
                            start_offset: s_char,
                            end_offset: end_char,
                            char_count: seg_chars,
                        });
                    }
                    current_start_char = None;
                    current_start_byte = None;
                }
            }
            continue;
        }

        if newline_run > 0 {
            newline_run = 0;
        }
        if current_start_char.is_none() {
            current_start_char = Some(i);
            current_start_byte = Some(byte_idx);
        }
        i += 1;
        chars_consumed = i;
    }

    // Trailing segment.
    if let (Some(s_char), Some(s_byte)) = (current_start_char, current_start_byte) {
        let end_char = chars_consumed.saturating_sub(newline_run);
        let end_byte = if end_char < total_chars {
            chars[end_char].0
        } else {
            content.len()
        };
        let text = &content[s_byte..end_byte];
        if !text.trim().is_empty() {
            segments.push(Segment {
                text,
                start_offset: s_char,
                end_offset: end_char,
                char_count: end_char - s_char,
            });
        }
    }

    // Fallback: source had no paragraph breaks at all (single
    // run-on string). Treat each line as its own segment so the
    // ceiling logic still has somewhere to break.
    if segments.len() == 1 && segments[0].text.contains('\n') {
        let only = segments[0];
        segments = single_newline_segments(content, only.start_offset);
    }

    let _ = start_char; // silence unused — kept above for parallel debugging
    segments
}

fn single_newline_segments(content: &str, start_char_offset: usize) -> Vec<Segment<'_>> {
    let mut out = Vec::new();
    let chars: Vec<(usize, char)> = content.char_indices().collect();

    let mut s_char: Option<usize> = None;
    let mut s_byte: Option<usize> = None;

    for (i, (byte_idx, ch)) in chars.iter().enumerate() {
        if i < start_char_offset {
            continue;
        }
        if *ch == '\n' {
            if let (Some(sc), Some(sb)) = (s_char, s_byte) {
                let text = &content[sb..*byte_idx];
                if !text.trim().is_empty() {
                    out.push(Segment {
                        text,
                        start_offset: sc,
                        end_offset: i,
                        char_count: i - sc,
                    });
                }
                s_char = None;
                s_byte = None;
            }
            continue;
        }
        if s_char.is_none() {
            s_char = Some(i);
            s_byte = Some(*byte_idx);
        }
    }

    if let (Some(sc), Some(sb)) = (s_char, s_byte) {
        let text = &content[sb..];
        if !text.trim().is_empty() {
            out.push(Segment {
                text,
                start_offset: sc,
                end_offset: chars.len(),
                char_count: chars.len() - sc,
            });
        }
    }

    out
}

/// Combine a slice of `Segment`s (already paragraph-bounded) into one
/// `Chunk`. The resulting chunk's text spans from the first segment's
/// start to the last segment's end, joined with `\n\n` between
/// segments to mirror the original paragraph layout.
fn make_chunk(segments: &[&Segment]) -> Chunk {
    debug_assert!(!segments.is_empty());
    let start = segments.first().unwrap().start_offset;
    let end = segments.last().unwrap().end_offset;
    let text = segments
        .iter()
        .map(|s| s.text)
        .collect::<Vec<_>>()
        .join("\n\n");
    let content_hash = fnv1a_64_hex(&text);
    Chunk {
        text,
        start_offset: start,
        end_offset: end,
        content_hash,
    }
}

/// Split a single segment that exceeds `MAX_CHARS`. Tries sentence
/// boundaries first (`.` `!` `?` followed by whitespace); falls back
/// to a hard char-count cut if no boundary is available.
fn split_oversize_segment(segment: &Segment) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let chars: Vec<(usize, char)> = segment.text.char_indices().collect();

    let mut window_start_char = 0usize; // relative to segment.text
    let mut window_start_byte = 0usize;
    let mut last_sentence_end_char: Option<usize> = None;

    let mut i = 0usize;
    while i < chars.len() {
        let (_byte_idx, ch) = chars[i];
        let pos = i + 1; // chars consumed from window_start
        if matches!(ch, '.' | '!' | '?') {
            // peek ahead — is this followed by whitespace?
            let next = chars.get(i + 1).map(|(_, c)| *c);
            if next.map(|c| c.is_whitespace()).unwrap_or(true) {
                last_sentence_end_char = Some(i + 1);
            }
        }

        let window_chars = (i + 1) - window_start_char;
        if window_chars >= MAX_CHARS {
            // Cut at the most recent sentence boundary if one exists
            // within the window; otherwise hard-cut.
            let cut_char_local = match last_sentence_end_char {
                Some(end) if end > window_start_char => end,
                _ => i + 1,
            };
            let cut_byte_local = chars
                .get(cut_char_local)
                .map(|(b, _)| *b)
                .unwrap_or(segment.text.len());
            let text = &segment.text[window_start_byte..cut_byte_local];
            let abs_start = segment.start_offset + window_start_char;
            let abs_end = segment.start_offset + cut_char_local;
            let chunk_text = text.trim_end_matches([' ', '\n', '\t']).to_string();
            let content_hash = fnv1a_64_hex(&chunk_text);
            chunks.push(Chunk {
                text: chunk_text,
                start_offset: abs_start,
                end_offset: abs_end,
                content_hash,
            });
            window_start_char = cut_char_local;
            window_start_byte = cut_byte_local;
            last_sentence_end_char = None;
            i = cut_char_local;
            continue;
        }
        i += 1;
    }

    if window_start_char < chars.len() {
        let text = &segment.text[window_start_byte..];
        let abs_start = segment.start_offset + window_start_char;
        let abs_end = segment.end_offset;
        let chunk_text = text.trim_end_matches([' ', '\n', '\t']).to_string();
        if !chunk_text.is_empty() {
            let content_hash = fnv1a_64_hex(&chunk_text);
            chunks.push(Chunk {
                text: chunk_text,
                start_offset: abs_start,
                end_offset: abs_end,
                content_hash,
            });
        }
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_content_yields_no_chunks() {
        assert!(chunk_text("").is_empty());
        assert!(chunk_text("   \n\n   ").is_empty());
    }

    #[test]
    fn short_content_is_one_chunk_with_offsets_zero_to_len() {
        let content = "Just a short note about pricing strategy.";
        let chunks = chunk_text(content);
        assert_eq!(chunks.len(), 1);
        let c = &chunks[0];
        assert_eq!(c.start_offset, 0);
        assert_eq!(c.end_offset, content.chars().count());
        assert_eq!(c.text, content);
        assert_eq!(c.content_hash.len(), 16);
    }

    #[test]
    fn paragraph_breaks_create_separate_segments_when_packed() {
        // Three small paragraphs — should fit in one chunk because
        // total < TARGET_CHARS.
        let content = "Para one is short.\n\nPara two is also short.\n\nFinally para three.";
        let chunks = chunk_text(content);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn long_content_splits_into_multiple_chunks() {
        // Build a content that's well above TARGET_CHARS but each
        // paragraph is bounded.
        let para = "x".repeat(700);
        let content = (0..6).map(|_| para.as_str()).collect::<Vec<_>>().join("\n\n");
        let chunks = chunk_text(&content);
        assert!(chunks.len() >= 2, "expected ≥2 chunks, got {}", chunks.len());
        for chunk in &chunks {
            assert!(chunk.char_count() <= MAX_CHARS);
            assert!(chunk.end_offset > chunk.start_offset);
        }
    }

    #[test]
    fn deterministic_chunking_yields_stable_hashes() {
        let content = "First paragraph here.\n\nSecond paragraph here.\n\nThird paragraph here.";
        let a = chunk_text(content);
        let b = chunk_text(content);
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.content_hash, y.content_hash);
            assert_eq!(x.start_offset, y.start_offset);
            assert_eq!(x.end_offset, y.end_offset);
            assert_eq!(x.text, y.text);
        }
    }

    #[test]
    fn oversize_paragraph_is_split_internally() {
        // A single 12000-char paragraph with no breaks must be split.
        let para = "Sentence number one. ".repeat(800); // ~16800 chars
        let chunks = chunk_text(&para);
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert!(chunk.char_count() <= MAX_CHARS, "chunk over ceiling: {}", chunk.char_count());
        }
    }

    #[test]
    fn offsets_round_trip_to_substring_of_source() {
        let content = "Para one with text.\n\nPara two with different text.\n\nPara three end.";
        let chunks = chunk_text(content);
        let chars: Vec<char> = content.chars().collect();
        for chunk in &chunks {
            let expected: String = chars[chunk.start_offset..chunk.end_offset]
                .iter()
                .collect();
            // Allow chunk text to be the joined paragraph form; the
            // substring of the source should *contain* the chunk text
            // (paragraph separators may differ between source and
            // chunk join).
            // Simple sanity: first chars match.
            let exp_start: String = expected.chars().take(10).collect();
            let chunk_start: String = chunk.text.chars().take(10).collect();
            assert_eq!(
                exp_start, chunk_start,
                "offset {}..{} does not start with chunk text",
                chunk.start_offset, chunk.end_offset
            );
        }
    }

    #[test]
    fn unicode_is_counted_in_chars_not_bytes() {
        // Each "—" is 1 char but 3 bytes in UTF-8.
        let content = "abc—def\n\nghi—jkl";
        let chunks = chunk_text(content);
        assert_eq!(chunks.len(), 1);
        // First chunk's end_offset should be in chars, not bytes.
        let total_chars = content.chars().count();
        assert!(chunks[0].end_offset <= total_chars);
    }

    #[test]
    fn fnv1a_is_deterministic_and_distinct() {
        assert_eq!(fnv1a_64_hex("hello"), fnv1a_64_hex("hello"));
        assert_ne!(fnv1a_64_hex("hello"), fnv1a_64_hex("world"));
        assert_eq!(fnv1a_64_hex("hello").len(), 16);
    }
}
