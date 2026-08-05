//! Bounded tool-output foundation (Phase 12).
//!
//! Tools can produce unbounded output. This module provides [`BoundedOutput`],
//! a size-capped representation of tool output: a UTF-8-safe `preview` plus
//! actionable metadata (total byte/line counts, whether/what was truncated,
//! how much was omitted) and optional typed access to a persisted artifact id
//! and to the full untruncated output.
//!
//! Filesystem artifact persistence is intentionally out of scope here: the
//! artifact id is a typed, opaque field that starts `None` and is only set by a
//! later phase that writes artifacts to disk.

/// Opaque, typed identifier for a persisted output artifact.
///
/// Persistence is not implemented in this phase; the wrapper exists so callers
/// can hold a concrete artifact id without special-casing an unwrapped
/// `String`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtifactId(String);

impl ArtifactId {
    /// Wrap an id string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrow the underlying id string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ArtifactId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Which limit (if any) forced the output to be cut down to a preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TruncationKind {
    None,
    Bytes,
    Lines,
    Both,
}

/// A byte/line-bounded representation of tool output.
///
/// `preview` is always valid UTF-8 and never exceeds the configured byte cap
/// or line cap. `total_bytes` / `total_lines` describe the original unbounded
/// output, so callers can report exactly how much was omitted. `artifact_id`
/// and `full_output` provide optional access to a persisted artifact and to the
/// complete payload respectively; both start `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedOutput {
    /// In-memory preview of the output, valid UTF-8, within all caps.
    pub preview: String,
    /// Total length in bytes of the original, unbounded output.
    pub total_bytes: usize,
    /// Total line count of the original, unbounded output.
    pub total_lines: usize,
    /// True when the preview is shorter than the original output.
    pub truncated: bool,
    /// Which limit(s) cut the output short.
    pub truncation: TruncationKind,
    /// Bytes removed to produce the preview.
    pub bytes_omitted: usize,
    /// Lines removed to produce the preview.
    pub lines_omitted: usize,
    /// Optional identifier of a persisted artifact.
    pub artifact_id: Option<ArtifactId>,
    /// Optional full, untruncated output.
    pub full_output: Option<String>,
}

/// Limits applied when building a [`BoundedOutput`] from raw output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    /// Maximum bytes allowed in the preview. `None` = no byte limit.
    pub max_bytes: Option<usize>,
    /// Maximum lines allowed in the preview. `None` = no line limit.
    pub max_lines: Option<usize>,
}

impl Default for Bounds {
    fn default() -> Self {
        Self {
            max_bytes: Some(4_096),
            max_lines: Some(200),
        }
    }
}

impl Bounds {
    /// No limits at all — the preview is the full output.
    pub fn unbounded() -> Self {
        Self {
            max_bytes: None,
            max_lines: None,
        }
    }
}

impl BoundedOutput {
    /// Construct a `BoundedOutput` from explicit preview + metadata.
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        preview: String,
        total_bytes: usize,
        total_lines: usize,
        truncated: bool,
        truncation: TruncationKind,
        bytes_omitted: usize,
        lines_omitted: usize,
    ) -> Self {
        Self {
            preview,
            total_bytes,
            total_lines,
            truncated,
            truncation,
            bytes_omitted,
            lines_omitted,
            artifact_id: None,
            full_output: None,
        }
    }

    /// Attach an artifact id to this output (builder).
    pub fn with_artifact_id(mut self, id: ArtifactId) -> Self {
        self.artifact_id = Some(id);
        self
    }

    /// Attach the full, untruncated output (builder).
    pub fn with_full_output(mut self, full: String) -> Self {
        self.full_output = Some(full);
        self
    }

    /// Build a bounded output from a raw string, applying the given [`Bounds`].
    ///
    /// The preview is valid UTF-8, never exceeds `max_bytes` or `max_lines`,
    /// and is cut on a UTF-8 char boundary. `full_output` is set when the
    pub fn truncate(input: &str, bounds: Bounds) -> Self {
        let total_bytes = input.len();
        let total_lines = input.lines().count();
        let line_limited = bounds.max_lines.is_some_and(|limit| total_lines > limit);

        // Line cap first: keep the first N lines and trim the trailing line
        // terminator of the Nth kept line so no dangling newline shows.
        let line_preview = match (bounds.max_lines, line_limited) {
            (Some(limit), true) => {
                let kept: String = input.split_inclusive('\n').take(limit).collect();
                kept.trim_end_matches(&['\n', '\r'][..]).to_string()
            }
            _ => input.to_string(),
        };
        let line_preview_len = line_preview.len();

        // Byte cap second: cut on a UTF-8 char boundary.
        let preview = match bounds.max_bytes {
            Some(limit) if line_preview_len > limit => {
                let mut end = limit;
                while end > 0 && !line_preview.is_char_boundary(end) {
                    end -= 1;
                }
                line_preview[..end].to_string()
            }
            _ => line_preview,
        };

        // byte_limited is true only when the byte cap shrank the preview below
        // the line-capped result; otherwise the line cap alone caused it.
        let byte_limited = preview.len() < line_preview_len;
        let truncated = preview.len() < total_bytes || line_limited;
        let truncation = match (byte_limited, line_limited) {
            (true, true) => TruncationKind::Both,
            (true, false) => TruncationKind::Bytes,
            (false, true) => TruncationKind::Lines,
            (false, false) => TruncationKind::None,
        };

        Self {
            preview: preview.clone(),
            total_bytes,
            total_lines,
            truncated,
            truncation,
            bytes_omitted: total_bytes.saturating_sub(preview.len()),
            lines_omitted: total_lines.saturating_sub(preview.lines().count()),
            artifact_id: None,
            full_output: truncated.then(|| input.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ArtifactId ──────────────────────────────────────────────────────────

    #[test]
    fn artifact_id_round_trips() {
        let id = ArtifactId::new("abc-123");
        assert_eq!(id.as_str(), "abc-123");
        assert_eq!(id.as_ref(), "abc-123");
    }

    #[test]
    fn artifact_id_equality_and_ordering() {
        let a = ArtifactId::new("a");
        let b = ArtifactId::new("a");
        let c = ArtifactId::new("b");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a < c);
    }

    // ── Bounds defaults ──────────────────────────────────────────────────────

    #[test]
    fn bounds_default_is_capped() {
        let b = Bounds::default();
        assert_eq!(b.max_bytes, Some(4_096));
        assert_eq!(b.max_lines, Some(200));
    }

    #[test]
    fn bounds_unbounded_is_none() {
        let b = Bounds::unbounded();
        assert!(b.max_bytes.is_none());
        assert!(b.max_lines.is_none());
    }

    // ── Small / no-truncation cases ───────────────────────────────────────────

    #[test]
    fn truncate_small_input_is_not_truncated() {
        let input = "hello world";
        let out = BoundedOutput::truncate(input, Bounds::default());
        assert!(!out.truncated);
        assert_eq!(out.truncation, TruncationKind::None);
        assert_eq!(out.preview, input);
        assert_eq!(out.total_bytes, input.len());
        assert_eq!(out.total_lines, 1);
        assert_eq!(out.bytes_omitted, 0);
        assert_eq!(out.lines_omitted, 0);
        assert!(out.full_output.is_none());
        assert!(out.artifact_id.is_none());
    }

    #[test]
    fn truncate_empty_input() {
        let out = BoundedOutput::truncate("", Bounds::default());
        assert!(!out.truncated);
        assert_eq!(out.preview, "");
        assert_eq!(out.total_bytes, 0);
        assert_eq!(out.total_lines, 0);
    }

    // ── Byte truncation ────────────────────────────────────────────────────────

    #[test]
    fn truncate_by_bytes_cuts_on_char_boundary() {
        // Each char is 2 bytes; cap at 5 bytes → must keep 2 chars (4 bytes),
        // not 2.5 chars.
        let input = "αβγδ"; // 8 bytes
        let bounds = Bounds {
            max_bytes: Some(5),
            max_lines: None,
        };
        let out = BoundedOutput::truncate(input, bounds);
        assert!(out.truncated);
        assert_eq!(out.truncation, TruncationKind::Bytes);
        assert_eq!(out.preview, "αβ");
        assert_eq!(out.preview.len(), 4);
        assert!(out.preview.len() <= 5);
        assert_eq!(out.total_bytes, 8);
        assert_eq!(out.bytes_omitted, 4);
        assert_eq!(out.full_output.as_deref(), Some(input));
    }

    #[test]
    fn truncate_by_bytes_ascii() {
        let input = "abcdefghij"; // 10 bytes
        let bounds = Bounds {
            max_bytes: Some(4),
            max_lines: None,
        };
        let out = BoundedOutput::truncate(input, bounds);
        assert!(out.truncated);
        assert_eq!(out.truncation, TruncationKind::Bytes);
        assert_eq!(out.preview, "abcd");
        assert_eq!(out.total_bytes, 10);
        assert_eq!(out.bytes_omitted, 6);
    }

    #[test]
    fn truncate_by_bytes_exactly_at_boundary_is_not_truncated() {
        let input = "abcdef"; // 6 bytes
        let bounds = Bounds {
            max_bytes: Some(6),
            max_lines: None,
        };
        let out = BoundedOutput::truncate(input, bounds);
        assert!(!out.truncated);
        assert_eq!(out.truncation, TruncationKind::None);
        assert_eq!(out.preview, input);
        assert_eq!(out.bytes_omitted, 0);
    }

    #[test]
    fn truncate_by_bytes_one_over_boundary() {
        let input = "abcdef"; // 6 bytes
        let bounds = Bounds {
            max_bytes: Some(5),
            max_lines: None,
        };
        let out = BoundedOutput::truncate(input, bounds);
        assert!(out.truncated);
        assert_eq!(out.preview, "abcde");
        assert_eq!(out.bytes_omitted, 1);
    }

    #[test]
    fn truncate_preserves_4_byte_emoji() {
        // 🚀 is 4 bytes; cap at 4 → keep exactly one emoji and truncate the rest.
        let input = "🚀🚀"; // 8 bytes
        let bounds = Bounds {
            max_bytes: Some(4),
            max_lines: None,
        };
        let out = BoundedOutput::truncate(input, bounds);
        assert!(out.truncated);
        assert_eq!(out.preview, "🚀");
    }

    #[test]
    fn truncate_4_byte_emoji_over_cap() {
        let input = "🚀🚀"; // 8 bytes
        let bounds = Bounds {
            max_bytes: Some(5),
            max_lines: None,
        };
        let out = BoundedOutput::truncate(input, bounds);
        assert!(out.truncated);
        assert_eq!(out.preview, "🚀"); // can't fit 2 emoji in 5 bytes
        assert_eq!(out.preview.len(), 4);
        assert_eq!(out.bytes_omitted, 4);
    }

    #[test]
    fn truncate_zero_byte_cap_yields_empty_preview() {
        let input = "abc";
        let bounds = Bounds {
            max_bytes: Some(0),
            max_lines: None,
        };
        let out = BoundedOutput::truncate(input, bounds);
        assert!(out.truncated);
        assert_eq!(out.preview, "");
        assert_eq!(out.total_bytes, 3);
        assert_eq!(out.bytes_omitted, 3);
    }

    // ── Line truncation ────────────────────────────────────────────────────────

    #[test]
    fn truncate_by_lines() {
        let input = "line1\nline2\nline3\nline4";
        let bounds = Bounds {
            max_bytes: None,
            max_lines: Some(2),
        };
        let out = BoundedOutput::truncate(input, bounds);
        assert!(out.truncated);
        assert_eq!(out.truncation, TruncationKind::Lines);
        assert_eq!(out.preview, "line1\nline2");
        assert_eq!(out.total_lines, 4);
        assert_eq!(out.lines_omitted, 2);
        assert_eq!(out.full_output.as_deref(), Some(input));
    }

    #[test]
    fn truncate_by_lines_exactly_at_count_is_not_truncated() {
        let input = "line1\nline2";
        let bounds = Bounds {
            max_bytes: None,
            max_lines: Some(2),
        };
        let out = BoundedOutput::truncate(input, bounds);
        assert!(!out.truncated);
        assert_eq!(out.preview, input);
        assert_eq!(out.lines_omitted, 0);
    }

    #[test]
    fn truncate_by_lines_with_trailing_newline() {
        // "a\nb\n" — .lines() yields ["a","b"] (2 lines); but the raw string
        // has a trailing newline. We define total_lines via str::lines().count().
        let input = "a\nb\n";
        let bounds = Bounds {
            max_bytes: None,
            max_lines: Some(2),
        };
        let out = BoundedOutput::truncate(input, bounds);
        assert!(!out.truncated);
        assert_eq!(out.total_lines, 2);
        // preview should contain all content up to the line cap; trailing
        // newline beyond the last kept line is trimmed.
        assert_eq!(out.preview, "a\nb\n");
    }

    #[test]
    fn truncate_by_lines_one_over() {
        let input = "a\nb\nc";
        let bounds = Bounds {
            max_bytes: None,
            max_lines: Some(2),
        };
        let out = BoundedOutput::truncate(input, bounds);
        assert!(out.truncated);
        assert_eq!(out.preview, "a\nb");
        assert_eq!(out.lines_omitted, 1);
    }

    #[test]
    fn truncate_zero_lines_yields_empty_preview() {
        let input = "a\nb";
        let bounds = Bounds {
            max_bytes: None,
            max_lines: Some(0),
        };
        let out = BoundedOutput::truncate(input, bounds);
        assert!(out.truncated);
        assert_eq!(out.preview, "");
        assert_eq!(out.total_lines, 2);
        assert_eq!(out.lines_omitted, 2);
    }

    // ── Both limits ────────────────────────────────────────────────────────────

    #[test]
    fn truncate_both_limits_line_cap_dominates() {
        // 3 short lines; byte cap is huge, line cap is 1.
        let input = "aaa\nbbb\nccc";
        let bounds = Bounds {
            max_bytes: Some(10_000),
            max_lines: Some(1),
        };
        let out = BoundedOutput::truncate(input, bounds);
        assert!(out.truncated);
        assert_eq!(out.truncation, TruncationKind::Lines);
        assert_eq!(out.preview, "aaa");
        assert_eq!(out.lines_omitted, 2);
    }

    #[test]
    fn truncate_both_limits_byte_cap_dominates() {
        // 2 lines, each 5 bytes; byte cap is 3.
        let input = "aaaaa\nbbbbb";
        let bounds = Bounds {
            max_bytes: Some(3),
            max_lines: Some(10_000),
        };
        let out = BoundedOutput::truncate(input, bounds);
        assert!(out.truncated);
        assert_eq!(out.truncation, TruncationKind::Bytes);
        assert_eq!(out.preview, "aaa");
        assert_eq!(out.bytes_omitted, 8); // 11 - 3
    }

    #[test]
    fn truncate_both_limits_both_active() {
        // 4 lines of 4 bytes each (plus newlines). Byte cap 6, line cap 2.
        // Line cap 2 → "aaaa\nbbbb" = 9 bytes > 6 → byte cap also kicks in.
        let input = "aaaa\nbbbb\ncccc\ndddd";
        let bounds = Bounds {
            max_bytes: Some(6),
            max_lines: Some(2),
        };
        let out = BoundedOutput::truncate(input, bounds);
        assert!(out.truncated);
        assert_eq!(out.truncation, TruncationKind::Both);
        // Line cap selects "aaaa\nbbbb" (9 bytes), byte cap then cuts to 6 → "aaaa\nb"
        assert_eq!(out.preview, "aaaa\nb");
        assert_eq!(out.preview.len(), 6);
    }

    // ── full_output only set when truncated ────────────────────────────────────

    #[test]
    fn full_output_not_set_when_not_truncated() {
        let input = "small";
        let out = BoundedOutput::truncate(input, Bounds::default());
        assert!(!out.truncated);
        assert!(out.full_output.is_none());
    }

    #[test]
    fn full_output_set_when_truncated() {
        let input = "abcdefghij";
        let bounds = Bounds {
            max_bytes: Some(3),
            max_lines: None,
        };
        let out = BoundedOutput::truncate(input, bounds);
        assert!(out.truncated);
        assert_eq!(out.full_output.as_deref(), Some(input));
    }

    // ── Builders ─────────────────────────────────────────────────────────────────

    #[test]
    fn from_parts_defaults_optional_fields_to_none() {
        let out = BoundedOutput::from_parts("prev".into(), 4, 1, false, TruncationKind::None, 0, 0);
        assert!(out.artifact_id.is_none());
        assert!(out.full_output.is_none());
    }

    #[test]
    fn with_artifact_id_sets_field() {
        let out = BoundedOutput::from_parts("p".into(), 1, 1, false, TruncationKind::None, 0, 0)
            .with_artifact_id(ArtifactId::new("art-1"));
        assert_eq!(
            out.artifact_id.as_ref().map(ArtifactId::as_str),
            Some("art-1")
        );
    }

    #[test]
    fn with_full_output_sets_field() {
        let out = BoundedOutput::from_parts("p".into(), 1, 1, false, TruncationKind::None, 0, 0)
            .with_full_output("full".into());
        assert_eq!(out.full_output.as_deref(), Some("full"));
    }

    #[test]
    fn builders_chain() {
        let out = BoundedOutput::from_parts("p".into(), 1, 1, false, TruncationKind::None, 0, 0)
            .with_artifact_id(ArtifactId::new("x"))
            .with_full_output("f".into());
        assert_eq!(out.artifact_id.as_ref().map(ArtifactId::as_str), Some("x"));
        assert_eq!(out.full_output.as_deref(), Some("f"));
    }

    // ── Unicode edge cases ───────────────────────────────────────────────────────

    #[test]
    fn truncate_mixed_ascii_unicode_byte_cap() {
        // "aéβ" = 1 + 2 + 2 = 5 bytes
        let input = "aéβ";
        let bounds = Bounds {
            max_bytes: Some(2),
            max_lines: None,
        };
        let out = BoundedOutput::truncate(input, bounds);
        assert!(out.truncated);
        assert_eq!(out.preview, "a"); // 'a' is 1 byte; 'é' is 2 bytes → total 3 > 2
        assert_eq!(out.preview.len(), 1);
        assert_eq!(out.total_bytes, 5);
        assert_eq!(out.bytes_omitted, 4);
    }

    #[test]
    fn truncate_crlf_line_endings() {
        // CRLF: str::lines() handles \r\n correctly.
        let input = "a\r\nb\r\nc";
        let bounds = Bounds {
            max_bytes: None,
            max_lines: Some(2),
        };
        let out = BoundedOutput::truncate(input, bounds);
        assert!(out.truncated);
        assert_eq!(out.total_lines, 3);
        // We keep the first 2 lines including their original line endings.
        assert_eq!(out.preview, "a\r\nb");
        assert_eq!(out.lines_omitted, 1);
    }

    #[test]
    fn truncate_unicode_lines_byte_cap_preserves_char_boundary() {
        // Two lines: "αβ\nγδ" — each line is 4 bytes; total 9 bytes (8 + \n).
        let input = "αβ\nγδ";
        let bounds = Bounds {
            max_bytes: Some(6),
            max_lines: None,
        };
        let out = BoundedOutput::truncate(input, bounds);
        assert!(out.truncated);
        // 6 bytes: "αβ\n" = 5 bytes, + "γ" = 7 > 6, so "αβ\n" only?
        // Actually "αβ\n" = 2+2+1 = 5 bytes; adding "γ" (2 bytes) = 7 > 6.
        // So preview = "αβ\n" (5 bytes).
        assert!(out.preview.len() <= 6);
        assert!(out.preview.is_char_boundary(out.preview.len()) || out.preview.is_empty());
        assert_eq!(out.total_bytes, 9);
    }

    #[test]
    fn truncate_preserves_valid_utf8() {
        // Fuzz-ish: build a string with mixed multibyte chars, truncate at
        // various byte caps, and assert preview is always valid UTF-8 and
        // never exceeds the cap.
        let chars = ['a', 'é', 'β', '🚀', '中', '\n'];
        let mut input = String::new();
        for _ in 0..100 {
            for &c in &chars {
                input.push(c);
            }
        }
        for cap in [1, 2, 3, 4, 5, 7, 10, 13, 50, 100, 1000] {
            let bounds = Bounds {
                max_bytes: Some(cap),
                max_lines: None,
            };
            let out = BoundedOutput::truncate(&input, bounds);
            assert!(
                out.preview.len() <= cap,
                "cap {} exceeded: preview len {}",
                cap,
                out.preview.len()
            );
            // preview is a String so it's always valid UTF-8 by construction;
            // additionally assert char boundary safety.
            assert!(out.preview.is_char_boundary(out.preview.len()));
            assert_eq!(out.total_bytes, input.len());
            if out.truncated {
                assert_eq!(out.bytes_omitted, input.len() - out.preview.len());
            }
        }
    }
}
