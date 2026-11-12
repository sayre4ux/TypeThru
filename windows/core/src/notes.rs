//! Release notes from CHANGELOG.md (built into the app, as on the Mac: `Sources/WhatsNew.swift`),
//! their inline styles, and the reading order of text recognized on screen (`ScreenText.layout`).

use crate::update_feed::{ChangelogEntry, is_newer, normalized};

/// Each "## " heading starts a version. Indented lines continue the line above.
pub fn parse_changelog(text: &str) -> Vec<ChangelogEntry> {
    let mut entries = Vec::new();
    let mut title: Option<String> = None;
    let mut lines: Vec<String> = Vec::new();
    let mut finish = |title: &Option<String>, lines: &mut Vec<String>| {
        if let Some(title) = title {
            let version = normalized(title);
            if !version.is_empty() {
                entries.push(ChangelogEntry { version, title: title.clone(), lines: std::mem::take(lines) });
            }
        }
        lines.clear();
    };
    for raw in text.replace("\r\n", "\n").split('\n') {
        if let Some(heading) = raw.strip_prefix("## ") {
            finish(&title, &mut lines);
            title = Some(heading.trim().to_owned());
        } else if title.is_some() && !raw.trim().is_empty() {
            match lines.last_mut() {
                Some(last) if raw.starts_with(' ') => {
                    last.push(' ');
                    last.push_str(raw.trim());
                }
                _ => lines.push(raw.to_owned()),
            }
        }
    }
    finish(&title, &mut lines);
    entries
}

/// Versions after `last` up to `current`, newest first (the changelog's order).
pub fn since<'a>(last: &str, current: &str, entries: &'a [ChangelogEntry]) -> Vec<&'a ChangelogEntry> {
    entries.iter().filter(|e| is_newer(&e.version, last) && !is_newer(&e.version, current)).collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Inline {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
}

/// Bold, italic, and code in one line of release notes. DECISION: no links; notes before an
/// update come from the internet. A [label](url) shows only its label.
pub fn parse_inline(line: &str) -> Vec<Inline> {
    let chars: Vec<char> = line.chars().collect();
    let find = |from: usize, target: char| chars.get(from..).and_then(|rest| rest.iter().position(|&c| c == target)).map(|p| p + from);
    let mut parts = Vec::new();
    let mut text = String::new();
    let (mut bold, mut italic) = (false, false);
    let flush = |parts: &mut Vec<Inline>, text: &mut String, bold: bool, italic: bool, code: bool| {
        if !text.is_empty() {
            parts.push(Inline { text: std::mem::take(text), bold: bold && !code, italic: italic && !code, code });
        }
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '`'
            && let Some(end) = find(i + 1, '`')
        {
            flush(&mut parts, &mut text, bold, italic, false);
            text.extend(&chars[i + 1..end]);
            flush(&mut parts, &mut text, bold, italic, true);
            i = end;
        } else if c == '*' && chars.get(i + 1) == Some(&'*') {
            flush(&mut parts, &mut text, bold, italic, false);
            bold = !bold;
            i += 1;
        } else if c == '*' {
            flush(&mut parts, &mut text, bold, italic, false);
            italic = !italic;
        } else if c == '['
            && let Some(mid) = (i..chars.len().saturating_sub(1)).find(|&k| chars[k] == ']' && chars[k + 1] == '(')
            && let Some(close) = find(mid, ')')
        {
            text.extend(&chars[i + 1..mid]);
            i = close;
        } else {
            text.push(c);
        }
        i += 1;
    }
    flush(&mut parts, &mut text, bold, italic, false);
    parts
}

/// A piece of recognized text and its box on screen, in pixels, origin at the top left.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Piece {
    fn mid_y(&self) -> f64 {
        self.y + self.height / 2.0
    }
    fn bottom(&self) -> f64 {
        self.y + self.height
    }
    fn right(&self) -> f64 {
        self.x + self.width
    }
}

/// Puts recognized pieces back in reading order: rows top to bottom, a blank line where a
/// paragraph gap was, and a tab between pieces far apart on one row (table columns).
pub fn layout(pieces: &[Piece]) -> String {
    let mut sorted: Vec<&Piece> = pieces.iter().filter(|p| !p.text.is_empty()).collect();
    sorted.sort_by(|a, b| a.mid_y().total_cmp(&b.mid_y()));
    let mut rows: Vec<Vec<&Piece>> = Vec::new();
    for piece in sorted {
        match rows.last_mut() {
            Some(row) if (row[0].mid_y() - piece.mid_y()).abs() < row[0].height.min(piece.height) / 2.0 => row.push(piece),
            _ => rows.push(vec![piece]),
        }
    }
    let mut text = String::new();
    let mut previous_bottom: Option<f64> = None;
    for mut row in rows {
        row.sort_by(|a, b| a.x.total_cmp(&b.x));
        let height = row.iter().map(|p| p.height).fold(f64::MIN, f64::max);
        if let Some(bottom) = previous_bottom {
            text.push('\n');
            let top = row.iter().map(|p| p.y).fold(f64::MAX, f64::min);
            if top - bottom > height * 0.9 {
                text.push('\n');
            }
        }
        for (i, piece) in row.iter().enumerate() {
            if i > 0 {
                text.push(if piece.x - row[i - 1].right() > height * 1.5 { '\t' } else { ' ' });
            }
            text.push_str(&piece.text);
        }
        previous_bottom = Some(row.iter().map(|p| p.bottom()).fold(f64::MIN, f64::max));
    }
    text
}
