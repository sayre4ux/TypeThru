//! The text rules shared with the Mac app (`Sources/main.swift`, `Typer`). Text is handled as
//! user-perceived characters (extended grapheme clusters), so "\r\n", "é", and emoji count as
//! one, as in Swift.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use unicode_segmentation::UnicodeSegmentation;

/// Formatting typed as Control plus a letter, as Word, Outlook, and most editors use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TextFormat {
    Bold,
    Italic,
}

impl TextFormat {
    pub fn key(self) -> char {
        match self {
            Self::Bold => 'b',
            Self::Italic => 'i',
        }
    }
}

/// What happens to Markdown marks such as **bold** and *italic* in the copied text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MarkdownMode {
    Keep,
    RemoveMarks,
    // DECISION: bold and italic by default; text from chat and AI tools uses Markdown.
    #[default]
    Format,
}

impl MarkdownMode {
    pub const ALL: [Self; 3] = [Self::Keep, Self::RemoveMarks, Self::Format];

    pub fn title(self) -> &'static str {
        match self {
            Self::Keep => "Type as is",
            Self::RemoveMarks => "Remove the marks",
            Self::Format => "Bold and italic",
        }
    }

    /// The name saved in settings.
    pub fn name(self) -> &'static str {
        match self {
            Self::Keep => "Keep",
            Self::RemoveMarks => "RemoveMarks",
            Self::Format => "Format",
        }
    }
}

/// The text as a list of user-perceived characters.
pub fn elements(text: &str) -> Vec<&str> {
    text.graphemes(true).collect()
}

pub fn is_line_break(element: &str) -> bool {
    matches!(element, "\n" | "\r" | "\r\n")
}

fn is_newline(element: &str) -> bool {
    is_line_break(element) || matches!(element, "\u{2028}" | "\u{2029}" | "\u{000B}" | "\u{000C}" | "\u{0085}")
}

/// A letter or number, as Swift's `Character.isLetter || isNumber`.
fn in_word(element: &str) -> bool {
    element.chars().next().is_some_and(char::is_alphanumeric)
}

fn is_whitespace(element: &str) -> bool {
    element.chars().all(char::is_whitespace)
}

/// Symbols with no key on common layouts, typed as plain keys that read the same.
/// DECISION: letters (such as é) are not replaced, because dropping an accent changes a word.
pub fn replacement(element: &str) -> Option<&'static str> {
    Some(match element {
        "•" | "◦" | "▪" | "▫" | "‣" | "⁃" | "●" | "○" | "■" | "□" => "-",
        "·" | "∙" | "‐" | "‑" | "‒" | "–" | "−" => "-",
        "—" | "―" => "--",
        "‘" | "’" | "‚" | "′" => "'",
        "“" | "”" | "„" | "″" | "«" | "»" => "\"",
        "…" => "...",
        "×" => "x",
        "÷" => "/",
        "→" => "->",
        "←" => "<-",
        "⇒" => "=>",
        "≤" => "<=",
        "≥" => ">=",
        "≠" => "!=",
        "©" => "(c)",
        "®" => "(R)",
        "™" => "(TM)",
        "\u{00A0}" | "\u{2002}" | "\u{2003}" | "\u{2009}" | "\u{202F}" => " ",
        "\u{200B}" | "\u{FEFF}" => "",
        // Line and paragraph separators, and Word's manual line break, become a normal line break.
        "\u{2028}" | "\u{2029}" | "\u{000B}" | "\u{0085}" => "\n",
        _ => return None,
    })
}

/// Replaces symbols that have no key, when every replacement character has one.
/// Returns the text and how many symbols were replaced.
pub fn replace_untypable(text: &str, typeable: impl Fn(&str) -> bool) -> (String, usize) {
    let mut result = String::with_capacity(text.len());
    let mut replaced = 0;
    for element in elements(text) {
        match replacement(element) {
            Some(substitute) if !typeable(element) && elements(substitute).into_iter().all(&typeable) => {
                result.push_str(substitute);
                replaced += 1;
            }
            _ => result.push_str(element),
        }
    }
    (result, replaced)
}

/// The characters that cannot be typed, in order, without duplicates.
pub fn unsupported(text: &str, typeable: impl Fn(&str) -> bool) -> Vec<String> {
    let mut seen = HashSet::new();
    elements(text).into_iter().filter(|e| !typeable(e) && seen.insert(*e)).map(str::to_owned).collect()
}

/// Leaves out line breaks at the end, so a copied command does not run by itself in a remote terminal.
pub fn without_trailing_line_breaks(text: &str) -> String {
    let mut elements = elements(text);
    while elements.last().is_some_and(|e| is_newline(e)) {
        elements.pop();
    }
    elements.concat()
}

/// Text with Markdown marks removed, and where formatting switches on or off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Formatted {
    pub text: String,
    /// By character position in `text`: the formats that switch there, in order.
    pub formats: BTreeMap<usize, Vec<TextFormat>>,
    /// How many marked spans were found.
    pub spans: usize,
}

/// Removes Markdown bold and italic marks and returns, by character position in the result,
/// where formatting switches on or off. Marks count only around text on one line and not
/// inside a word, so 2**8, 5 * 3, *.txt, and **kwargs stay as they are.
pub fn markdown(text: &str) -> Formatted {
    let chars = elements(text);
    let n = chars.len();
    let mut removed = vec![false; n];
    let mut styles: Vec<BTreeSet<TextFormat>> = vec![BTreeSet::new(); n];
    let mut spans = 0;

    let word = |i: isize| i >= 0 && (i as usize) < n && in_word(chars[i as usize]);
    let space = |i: usize| is_whitespace(chars[i]);

    // DECISION: asterisks only; underscores are common in code and file names (__init__).
    // Longest marks first, so ***both*** and **bold** are not read as italics.
    let marks: [(usize, &[TextFormat]); 3] = [(3, &[TextFormat::Bold, TextFormat::Italic]), (2, &[TextFormat::Bold]), (1, &[TextFormat::Italic])];
    for (m, format) in marks {
        let runs_on = |removed: &[bool], i: isize| i >= 0 && (i as usize) < n && chars[i as usize] == "*" && !removed[i as usize];
        let mark = |removed: &[bool], i: usize| i + m <= n && (0..m).all(|k| chars[i + k] == "*" && !removed[i + k]);
        let mut i = 0;
        while i + m < n {
            let at = i as isize;
            if !mark(&removed, i) || word(at - 1) || runs_on(&removed, at - 1) || runs_on(&removed, at + m as isize) || space(i + m) {
                i += 1;
                continue;
            }
            let mut close = None;
            let mut j = i + m + 1;
            while j + m <= n && !is_newline(chars[j - 1]) {
                let jt = j as isize;
                if mark(&removed, j) && !space(j - 1) && !runs_on(&removed, jt - 1) && !runs_on(&removed, jt + m as isize) && !word(jt + m as isize) {
                    close = Some(j);
                    break;
                }
                j += 1;
            }
            let Some(c) = close else {
                i += 1;
                continue;
            };
            for k in 0..m {
                removed[i + k] = true;
                removed[c + k] = true;
            }
            for k in i + m..c {
                if !removed[k] {
                    styles[k].extend(format.iter().copied());
                }
            }
            spans += 1;
            i = c + m;
        }
    }

    let mut result = String::with_capacity(text.len());
    let mut count = 0;
    let mut formats = BTreeMap::new();
    let mut current = BTreeSet::new();
    for k in 0..n {
        if removed[k] {
            continue;
        }
        if styles[k] != current {
            formats.insert(count, current.symmetric_difference(&styles[k]).copied().collect());
            current = styles[k].clone();
        }
        result.push_str(chars[k]);
        count += 1;
    }
    if !current.is_empty() {
        formats.insert(count, current.into_iter().collect());
    }
    Formatted { text: result, formats, spans }
}
