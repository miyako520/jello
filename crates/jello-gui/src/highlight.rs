use std::ops::Range;

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{Color32, FontId, Stroke};

const MAX_HIGHLIGHT_SPANS: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlayState {
    Pending,
    Changed,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OverlayRange {
    pub(crate) range: Range<usize>,
    pub(crate) state: OverlayState,
    pub(crate) selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenClass {
    Plain,
    Punctuation,
    String,
    Number,
    Keyword,
    Identifier,
    Comment,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenSpan {
    pub range: Range<usize>,
    pub class: TokenClass,
}

pub fn token_spans(source: &str) -> Vec<TokenSpan> {
    if source.len() > jello::MAX_INPUT_BYTES {
        return vec![TokenSpan {
            range: 0..source.len(),
            class: TokenClass::Plain,
        }];
    }
    let bytes = source.as_bytes();
    let mut spans = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let start = index;
        let class = match bytes[index] {
            b' ' | b'\t' | b'\r' | b'\n' => {
                index += 1;
                while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                    index += 1;
                }
                TokenClass::Plain
            }
            b'{' | b'}' | b'[' | b']' | b':' | b',' => {
                index += 1;
                TokenClass::Punctuation
            }
            b'"' | b'\'' => {
                let quote = bytes[index];
                index += 1;
                let mut escaped = false;
                let mut terminated = false;
                while index < bytes.len() {
                    let byte = bytes[index];
                    index += 1;
                    if escaped {
                        escaped = false;
                    } else if byte == b'\\' {
                        escaped = true;
                    } else if byte == quote {
                        terminated = true;
                        break;
                    }
                }
                if terminated {
                    TokenClass::String
                } else {
                    TokenClass::Invalid
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < bytes.len() && !matches!(bytes[index], b'\r' | b'\n') {
                    index += 1;
                }
                TokenClass::Comment
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                let mut terminated = false;
                while index + 1 < bytes.len() {
                    if bytes[index] == b'*' && bytes[index + 1] == b'/' {
                        index += 2;
                        terminated = true;
                        break;
                    }
                    index += 1;
                }
                if !terminated {
                    index = bytes.len();
                    TokenClass::Invalid
                } else {
                    TokenClass::Comment
                }
            }
            b'-' | b'+' | b'.' | b'0'..=b'9' => {
                index += 1;
                while index < bytes.len()
                    && matches!(
                        bytes[index],
                        b'0'..=b'9'
                            | b'a'..=b'f'
                            | b'A'..=b'F'
                            | b'x'
                            | b'X'
                            | b'+'
                            | b'-'
                            | b'.'
                    )
                {
                    index += 1;
                }
                TokenClass::Number
            }
            _ => {
                let first = source[index..].chars().next().unwrap();
                index += first.len_utf8();
                while index < bytes.len() {
                    let ch = source[index..].chars().next().unwrap();
                    if !(ch == '_' || ch == '$' || ch.is_alphanumeric()) {
                        break;
                    }
                    index += ch.len_utf8();
                }
                match &source[start..index] {
                    "true" | "false" | "null" => TokenClass::Keyword,
                    word if word
                        .chars()
                        .all(|ch| ch == '_' || ch == '$' || ch.is_alphanumeric()) =>
                    {
                        TokenClass::Identifier
                    }
                    _ => TokenClass::Invalid,
                }
            }
        };
        if spans.len() == MAX_HIGHLIGHT_SPANS {
            return vec![TokenSpan {
                range: 0..source.len(),
                class: TokenClass::Plain,
            }];
        }
        spans.push(TokenSpan {
            range: start..index,
            class,
        });
    }
    spans
}

/// A content-addressed cache of token spans so repaint frames do not
/// re-tokenize text that has not changed.
#[derive(Default)]
pub(crate) struct TokenCache {
    content: String,
    spans: Vec<TokenSpan>,
}

impl TokenCache {
    pub(crate) fn spans_for(&mut self, source: &str) -> &[TokenSpan] {
        if self.content != source {
            self.spans = token_spans(source);
            self.content = source.to_string();
        }
        &self.spans
    }
}

pub(crate) fn layout_job_with_spans(
    source: &str,
    spans: &[TokenSpan],
    dark: bool,
    overlays: &[OverlayRange],
) -> LayoutJob {
    let mut overlays = normalized_overlays(source, overlays);
    overlays.sort_by_key(|overlay| overlay.range.start);
    let mut boundaries = Vec::with_capacity(2 + spans.len() * 2 + overlays.len() * 2);
    boundaries.push(0);
    boundaries.push(source.len());
    for span in spans {
        boundaries.push(span.range.start);
        boundaries.push(span.range.end);
    }
    for overlay in &overlays {
        boundaries.push(overlay.range.start);
        boundaries.push(overlay.range.end);
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let mut active: Vec<&OverlayRange> = Vec::new();
    let mut next_overlay = 0;
    let mut span_index = 0;
    for pair in boundaries.windows(2) {
        let range = pair[0]..pair[1];
        if range.is_empty() {
            continue;
        }
        while span_index + 1 < spans.len() && spans[span_index].range.end <= range.start {
            span_index += 1;
        }
        let span = &spans[span_index];
        while next_overlay < overlays.len() && overlays[next_overlay].range.start <= range.start {
            active.push(&overlays[next_overlay]);
            next_overlay += 1;
        }
        active.retain(|overlay| overlay.range.end > range.start);
        let overlay = active
            .iter()
            .max_by_key(|overlay| overlay_priority(overlay))
            .copied();
        job.append(&source[range], 0.0, text_format(dark, span.class, overlay));
    }
    job
}

fn normalized_overlays(source: &str, overlays: &[OverlayRange]) -> Vec<OverlayRange> {
    overlays
        .iter()
        .filter_map(|overlay| {
            let start = floor_char_boundary(source, overlay.range.start.min(source.len()));
            let end = ceil_char_boundary(source, overlay.range.end.min(source.len()));
            (start < end).then_some(OverlayRange {
                range: start..end,
                state: overlay.state,
                selected: overlay.selected,
            })
        })
        .collect()
}

fn floor_char_boundary(source: &str, mut index: usize) -> usize {
    while index > 0 && !source.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(source: &str, mut index: usize) -> usize {
    while index < source.len() && !source.is_char_boundary(index) {
        index += 1;
    }
    index
}

fn overlay_priority(overlay: &OverlayRange) -> u8 {
    let state = match overlay.state {
        OverlayState::Pending => 1,
        OverlayState::Changed => 2,
        OverlayState::Accepted => 2,
        OverlayState::Rejected => 3,
    };
    state + u8::from(overlay.selected) * 4
}

fn text_format(dark: bool, class: TokenClass, overlay: Option<&OverlayRange>) -> TextFormat {
    let mut format = TextFormat {
        font_id: FontId::monospace(14.0),
        color: syntax_color(dark, class),
        ..Default::default()
    };
    let Some(overlay) = overlay else {
        return format;
    };
    format.background = overlay_color(dark, overlay.state, overlay.selected);
    if overlay.state == OverlayState::Changed {
        format.underline = Stroke::new(
            if overlay.selected { 1.5 } else { 1.25 },
            overlay_underline_color(dark, overlay.state),
        );
    } else if overlay.selected {
        format.underline = Stroke::new(1.5, overlay_underline_color(dark, overlay.state));
    }
    format
}

fn syntax_color(dark: bool, class: TokenClass) -> Color32 {
    match (dark, class) {
        (_, TokenClass::Plain) => Color32::GRAY,
        (false, TokenClass::Punctuation) => Color32::from_rgb(55, 65, 81),
        (true, TokenClass::Punctuation) => Color32::from_rgb(210, 215, 225),
        (false, TokenClass::String) => Color32::from_rgb(23, 107, 77),
        (true, TokenClass::String) => Color32::from_rgb(120, 205, 155),
        (false, TokenClass::Number) => Color32::from_rgb(162, 75, 25),
        (true, TokenClass::Number) => Color32::from_rgb(240, 170, 105),
        (false, TokenClass::Keyword) => Color32::from_rgb(37, 99, 235),
        (true, TokenClass::Keyword) => Color32::from_rgb(120, 165, 255),
        (false, TokenClass::Identifier) => Color32::from_rgb(126, 65, 155),
        (true, TokenClass::Identifier) => Color32::from_rgb(205, 150, 235),
        (_, TokenClass::Comment) => Color32::from_rgb(105, 125, 110),
        (_, TokenClass::Invalid) => Color32::from_rgb(220, 50, 47),
    }
}

fn overlay_color(dark: bool, state: OverlayState, selected: bool) -> Color32 {
    let alpha = if selected { 160 } else { 60 };
    let (red, green, blue) = match (dark, state) {
        (false, OverlayState::Pending) => (234, 179, 8),
        (false, OverlayState::Changed) => (59, 130, 246),
        (false, OverlayState::Accepted) => (34, 197, 94),
        (false, OverlayState::Rejected) => (239, 68, 68),
        (true, OverlayState::Pending) => (168, 136, 28),
        (true, OverlayState::Changed) => (80, 118, 208),
        (true, OverlayState::Accepted) => (48, 138, 84),
        (true, OverlayState::Rejected) => (186, 64, 58),
    };
    Color32::from_rgba_unmultiplied(red, green, blue, alpha)
}

fn overlay_underline_color(dark: bool, state: OverlayState) -> Color32 {
    match (dark, state) {
        (false, OverlayState::Pending) => Color32::from_rgb(180, 120, 0),
        (false, OverlayState::Changed) => Color32::from_rgb(37, 99, 235),
        (false, OverlayState::Accepted) => Color32::from_rgb(22, 130, 65),
        (false, OverlayState::Rejected) => Color32::from_rgb(220, 50, 47),
        (true, OverlayState::Pending) => Color32::from_rgb(210, 160, 40),
        (true, OverlayState::Changed) => Color32::from_rgb(100, 140, 235),
        (true, OverlayState::Accepted) => Color32::from_rgb(70, 160, 100),
        (true, OverlayState::Rejected) => Color32::from_rgb(235, 90, 80),
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::Color32;
    use eframe::epaint::text::ByteRangeExt;

    use super::{
        MAX_HIGHLIGHT_SPANS, OverlayRange, OverlayState, TokenCache, TokenClass,
        layout_job_with_spans, overlay_color, overlay_underline_color, token_spans,
    };

    #[test]
    fn fragmented_large_input_degrades_to_one_plain_span() {
        let source = "@".repeat(MAX_HIGHLIGHT_SPANS + 1);

        let spans = token_spans(&source);

        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].range, 0..source.len());
        assert_eq!(spans[0].class, TokenClass::Plain);
    }

    #[test]
    fn token_cache_reuses_spans_for_unchanged_source() {
        let source = r#"{"name":"Ada"}"#;
        let mut cache = TokenCache::default();
        let ranges = |cache: &mut TokenCache| {
            cache
                .spans_for(source)
                .iter()
                .map(|span| span.range.clone())
                .collect::<Vec<_>>()
        };

        assert_eq!(ranges(&mut cache), ranges(&mut cache));
        assert_eq!(ranges(&mut cache).len(), 5);

        let changed: Vec<_> = cache
            .spans_for("[]")
            .iter()
            .map(|span| span.range.clone())
            .collect();
        assert_eq!(changed, vec![0..1, 1..2]);
    }

    #[test]
    fn repair_overlays_preserve_text_and_split_token_spans() {
        let source = r#"{"name":"Ada"}"#;
        let overlays = [OverlayRange {
            range: 1..7,
            state: OverlayState::Pending,
            selected: true,
        }];

        let job = layout_job_with_spans(source, &token_spans(source), false, &overlays);

        assert_eq!(
            job.sections
                .iter()
                .map(|section| &source[section.byte_range.as_usize()])
                .collect::<String>(),
            source
        );
        assert!(
            job.sections
                .iter()
                .any(|section| { section.format.background != Color32::TRANSPARENT })
        );
    }

    #[test]
    fn repair_overlays_clamp_unicode_ranges_to_character_boundaries() {
        let source = r#"{"??":"??"}"#;
        let overlays = [OverlayRange {
            range: 2..source.len() - 1,
            state: OverlayState::Accepted,
            selected: false,
        }];

        let job = layout_job_with_spans(source, &token_spans(source), false, &overlays);

        assert_eq!(
            job.sections
                .iter()
                .map(|section| &source[section.byte_range.as_usize()])
                .collect::<String>(),
            source
        );
        assert!(job.sections.iter().all(|section| {
            source.is_char_boundary(section.byte_range.start.0)
                && source.is_char_boundary(section.byte_range.end.0)
        }));
    }

    #[test]
    fn every_overlay_state_has_visible_colors_in_both_themes() {
        for state in [
            OverlayState::Pending,
            OverlayState::Changed,
            OverlayState::Accepted,
            OverlayState::Rejected,
        ] {
            for dark in [false, true] {
                for selected in [false, true] {
                    let color = overlay_color(dark, state, selected);
                    assert!(
                        color.a() > 0,
                        "{state:?} must be visible in {} theme",
                        if dark { "dark" } else { "light" }
                    );
                    let underline = overlay_underline_color(dark, state);
                    assert!(
                        underline.a() > 0,
                        "{state:?} underline must be visible in {} theme",
                        if dark { "dark" } else { "light" }
                    );
                }
            }
        }
    }

    #[test]
    fn highlighter_recognizes_json_tokens_without_changing_text() {
        let source = r#"{"name":"Ada","age":42,"ready":true,"empty":null}"#;
        let spans = token_spans(source);

        assert_eq!(spans[0].class, TokenClass::Punctuation);
        assert!(spans.iter().any(|span| span.class == TokenClass::String));
        assert!(spans.iter().any(|span| span.class == TokenClass::Number));
        assert!(spans.iter().any(|span| span.class == TokenClass::Keyword));
        assert_eq!(
            spans
                .iter()
                .map(|span| &source[span.range.clone()])
                .collect::<String>(),
            source
        );
    }

    #[test]
    fn unterminated_string_is_kept_as_a_single_visible_span() {
        let source = r#"{"name":"Ada}"#;
        let spans = token_spans(source);

        assert_eq!(spans.last().unwrap().class, TokenClass::Invalid);
        assert_eq!(
            spans
                .iter()
                .map(|span| &source[span.range.clone()])
                .collect::<String>(),
            source
        );
    }
}
