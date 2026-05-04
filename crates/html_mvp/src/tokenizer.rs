#![deny(unsafe_code)]
#![allow(clippy::too_many_lines)]

//! HTML tokenizer for Sylphos' HTML5-lite parser.
//!
//! This is not a full WHATWG tokenizer, because then this module would become a
//! lifestyle choice. It does implement the pieces Sylphos needs for real site
//! compatibility work: comments, doctypes, start/end tags, boolean attributes,
//! raw-text/RCDATA handling, numeric/named entities, and malformed-tag recovery.

/// Token emitted by the HTML tokenizer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// `<!doctype ...>`.
    Doctype { name: String },

    /// Start tag.
    StartTag {
        name: String,
        attrs: Vec<(String, String)>,
        self_closing: bool,
    },

    /// End tag.
    EndTag { name: String },

    /// Comment node.
    Comment(String),

    /// Text node.
    Text(String),
}

/// HTML tokenizer.
pub struct Tokenizer<'a> {
    input: &'a str,
}

impl<'a> Tokenizer<'a> {
    /// Creates a tokenizer.
    #[must_use]
    pub const fn new(input: &'a str) -> Self {
        Self { input }
    }

    /// Tokenizes input into a small HTML token stream.
    #[must_use]
    pub fn tokenize(self) -> Vec<Token> {
        let chars = self.input.chars().collect::<Vec<_>>();
        let mut tokens = Vec::new();
        let mut text = String::new();
        let mut index = 0usize;

        while index < chars.len() {
            if chars[index] != '<' {
                text.push(chars[index]);
                index += 1;
                continue;
            }

            if starts_with(&chars, index, "<!--") {
                if let Some(end) = find_sequence(&chars, index + 4, "-->") {
                    flush_decoded_text(&mut tokens, &mut text);
                    tokens.push(Token::Comment(chars[index + 4..end].iter().collect()));
                    index = end + 3;
                    continue;
                }

                // Broken comments consume the rest as comment text. This mirrors
                // browser tolerance better than throwing away the page.
                flush_decoded_text(&mut tokens, &mut text);
                tokens.push(Token::Comment(chars[index + 4..].iter().collect()));
                break;
            }

            if starts_with(&chars, index, "<!") {
                if let Some(end) = find_char(&chars, index + 2, '>') {
                    flush_decoded_text(&mut tokens, &mut text);
                    let raw = chars[index + 2..end].iter().collect::<String>();
                    let name = normalize_doctype_name(&raw);
                    if !name.is_empty() {
                        tokens.push(Token::Doctype { name });
                    }
                    index = end + 1;
                    continue;
                }

                text.push('<');
                index += 1;
                continue;
            }

            if starts_with(&chars, index, "</") {
                if let Some((name, next_index)) = parse_end_tag(&chars, index) {
                    flush_decoded_text(&mut tokens, &mut text);
                    tokens.push(Token::EndTag { name });
                    index = next_index;
                    continue;
                }

                text.push('<');
                index += 1;
                continue;
            }

            if index + 1 < chars.len() && is_tag_name_char(chars[index + 1]) {
                if let Some((token, next_index)) = parse_start_tag(&chars, index) {
                    flush_decoded_text(&mut tokens, &mut text);

                    let raw_mode = match &token {
                        Token::StartTag {
                            name, self_closing, ..
                        } if !self_closing => RawTextMode::for_tag(name),
                        _ => RawTextMode::None,
                    };

                    tokens.push(token);
                    index = next_index;

                    match raw_mode {
                        RawTextMode::None => {}
                        RawTextMode::PlainText => {
                            let raw = chars[index..].iter().collect::<String>();
                            if !raw.is_empty() {
                                tokens.push(Token::Text(raw));
                            }
                            break;
                        }
                        RawTextMode::RawText(tag) | RawTextMode::RcData(tag) => {
                            let close = format!("</{tag}");
                            if let Some(close_index) =
                                find_case_insensitive_sequence(&chars, index, &close)
                            {
                                let raw = chars[index..close_index].iter().collect::<String>();
                                if !raw.is_empty() {
                                    let value = if raw_mode.is_rcdata() {
                                        decode_entities(&raw)
                                    } else {
                                        raw
                                    };
                                    tokens.push(Token::Text(value));
                                }

                                if let Some((end_name, after_end)) =
                                    parse_end_tag(&chars, close_index)
                                {
                                    tokens.push(Token::EndTag { name: end_name });
                                    index = after_end;
                                } else {
                                    index = close_index;
                                }
                            } else {
                                let raw = chars[index..].iter().collect::<String>();
                                if !raw.is_empty() {
                                    let value = if raw_mode.is_rcdata() {
                                        decode_entities(&raw)
                                    } else {
                                        raw
                                    };
                                    tokens.push(Token::Text(value));
                                }
                                break;
                            }
                        }
                    }

                    continue;
                }

                text.push('<');
                index += 1;
                continue;
            }

            text.push('<');
            index += 1;
        }

        flush_decoded_text(&mut tokens, &mut text);
        tokens
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RawTextMode {
    None,
    RawText(&'static str),
    RcData(&'static str),
    PlainText,
}

impl RawTextMode {
    fn for_tag(tag: &str) -> Self {
        match tag {
            "script" | "style" | "xmp" | "iframe" | "noembed" | "noframes" => {
                Self::RawText(static_tag(tag))
            }
            "title" | "textarea" => Self::RcData(static_tag(tag)),
            "plaintext" => Self::PlainText,
            _ => Self::None,
        }
    }

    const fn is_rcdata(&self) -> bool {
        matches!(self, Self::RcData(_))
    }
}

fn static_tag(tag: &str) -> &'static str {
    match tag {
        "script" => "script",
        "style" => "style",
        "xmp" => "xmp",
        "iframe" => "iframe",
        "noembed" => "noembed",
        "noframes" => "noframes",
        "title" => "title",
        "textarea" => "textarea",
        _ => "",
    }
}

fn parse_start_tag(chars: &[char], start: usize) -> Option<(Token, usize)> {
    let mut index = start + 1;
    let name_start = index;

    while index < chars.len() && is_tag_name_char(chars[index]) {
        index += 1;
    }

    if name_start == index {
        return None;
    }

    let name = chars[name_start..index]
        .iter()
        .collect::<String>()
        .to_ascii_lowercase();

    let mut attrs = Vec::new();
    let mut self_closing = false;

    loop {
        index = skip_spaces(chars, index);

        if index >= chars.len() {
            return None;
        }

        match chars[index] {
            '>' => {
                index += 1;
                break;
            }
            '/' => {
                if index + 1 < chars.len() && chars[index + 1] == '>' {
                    self_closing = true;
                    index += 2;
                    break;
                }

                // Treat stray slash as an attribute-ish separator and recover.
                index += 1;
            }
            _ => {
                let (attr, next_index) = parse_attr(chars, index)?;
                if !attr.0.is_empty() && !attrs.iter().any(|(name, _)| name == &attr.0) {
                    attrs.push(attr);
                }
                index = next_index;
            }
        }
    }

    Some((
        Token::StartTag {
            name,
            attrs,
            self_closing,
        },
        index,
    ))
}

fn parse_end_tag(chars: &[char], start: usize) -> Option<(String, usize)> {
    let mut index = start + 2;
    index = skip_spaces(chars, index);

    let name_start = index;

    while index < chars.len() && is_tag_name_char(chars[index]) {
        index += 1;
    }

    if name_start == index {
        return None;
    }

    let name = chars[name_start..index]
        .iter()
        .collect::<String>()
        .to_ascii_lowercase();

    while index < chars.len() && chars[index] != '>' {
        index += 1;
    }

    (index < chars.len()).then_some((name, index + 1))
}

fn parse_attr(chars: &[char], start: usize) -> Option<((String, String), usize)> {
    let mut index = start;
    let name_start = index;

    while index < chars.len() && is_attr_name_char(chars[index]) {
        index += 1;
    }

    if name_start == index {
        return None;
    }

    let name = chars[name_start..index]
        .iter()
        .collect::<String>()
        .to_ascii_lowercase();

    index = skip_spaces(chars, index);

    if index >= chars.len() || chars[index] != '=' {
        return Some(((name, String::new()), index));
    }

    index += 1;
    index = skip_spaces(chars, index);

    if index >= chars.len() {
        return Some(((name, String::new()), index));
    }

    let value;

    match chars[index] {
        '"' | '\'' => {
            let quote = chars[index];
            index += 1;
            let value_start = index;

            while index < chars.len() && chars[index] != quote {
                index += 1;
            }

            value = chars[value_start..index.min(chars.len())]
                .iter()
                .collect::<String>();

            if index < chars.len() {
                index += 1;
            }
        }
        _ => {
            let value_start = index;

            while index < chars.len()
                && !is_space(chars[index])
                && chars[index] != '>'
                && !(chars[index] == '/' && index + 1 < chars.len() && chars[index + 1] == '>')
            {
                index += 1;
            }

            value = chars[value_start..index].iter().collect::<String>();
        }
    }

    Some(((name, decode_entities(&value)), index))
}

fn flush_decoded_text(tokens: &mut Vec<Token>, text: &mut String) {
    if text.is_empty() {
        return;
    }

    tokens.push(Token::Text(decode_entities(text)));
    text.clear();
}

fn normalize_doctype_name(raw: &str) -> String {
    let trimmed = raw.trim();

    if trimmed.len() >= "doctype".len()
        && trimmed[.."doctype".len()].eq_ignore_ascii_case("doctype")
    {
        let value = trimmed["doctype".len()..].trim();
        return if value.is_empty() {
            "html".to_owned()
        } else {
            value
                .split_whitespace()
                .next()
                .unwrap_or("html")
                .to_ascii_lowercase()
        };
    }

    trimmed.to_ascii_lowercase()
}

fn starts_with(chars: &[char], start: usize, pattern: &str) -> bool {
    let pattern_chars = pattern.chars().collect::<Vec<_>>();
    start + pattern_chars.len() <= chars.len()
        && chars[start..start + pattern_chars.len()] == pattern_chars
}

fn find_sequence(chars: &[char], start: usize, pattern: &str) -> Option<usize> {
    let pattern_chars = pattern.chars().collect::<Vec<_>>();
    if pattern_chars.is_empty() || start >= chars.len() {
        return None;
    }

    let mut index = start;
    while index + pattern_chars.len() <= chars.len() {
        if chars[index..index + pattern_chars.len()] == pattern_chars {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn find_case_insensitive_sequence(chars: &[char], start: usize, pattern: &str) -> Option<usize> {
    let pattern = pattern
        .chars()
        .map(|ch| ch.to_ascii_lowercase())
        .collect::<Vec<_>>();
    if pattern.is_empty() || start >= chars.len() {
        return None;
    }

    let mut index = start;
    while index + pattern.len() <= chars.len() {
        let matches = chars[index..index + pattern.len()]
            .iter()
            .zip(pattern.iter())
            .all(|(left, right)| left.to_ascii_lowercase() == *right);
        if matches {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn find_char(chars: &[char], start: usize, needle: char) -> Option<usize> {
    chars
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, value)| (*value == needle).then_some(index))
}

fn skip_spaces(chars: &[char], mut index: usize) -> usize {
    while index < chars.len() && is_space(chars[index]) {
        index += 1;
    }
    index
}

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\n' | '\t' | '\r' | '\x0C')
}

fn is_tag_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':')
}

fn is_attr_name_char(c: char) -> bool {
    !is_space(c) && !matches!(c, '=' | '/' | '>' | '<' | '"' | '\'')
}

/// Decodes HTML named and numeric character references.
#[must_use]
pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '&' {
            out.push(ch);
            continue;
        }

        let mut name = String::new();
        while let Some(&next) = chars.peek() {
            if next.is_whitespace() || next == '<' || name.len() > 32 {
                break;
            }
            name.push(next);
            let _ = chars.next();
            if next == ';' {
                break;
            }
        }

        if name.is_empty() {
            out.push('&');
            continue;
        }

        if let Some(value) = decode_entity_name(&name) {
            out.push_str(&value);
        } else {
            out.push('&');
            out.push_str(&name);
        }
    }

    out
}

fn decode_entity_name(name: &str) -> Option<String> {
    let trimmed = name.strip_suffix(';').unwrap_or(name);
    let value = match trimmed {
        "lt" => "<",
        "gt" => ">",
        "amp" => "&",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => "\u{00A0}",
        "copy" => "\u{00A9}",
        "reg" => "\u{00AE}",
        "trade" => "\u{2122}",
        "hellip" => "\u{2026}",
        "mdash" => "\u{2014}",
        "ndash" => "\u{2013}",
        "lsquo" => "\u{2018}",
        "rsquo" => "\u{2019}",
        "ldquo" => "\u{201C}",
        "rdquo" => "\u{201D}",
        "euro" => "\u{20AC}",
        _ => return decode_numeric_entity(trimmed),
    };
    Some(value.to_owned())
}

fn decode_numeric_entity(name: &str) -> Option<String> {
    let body = name.strip_prefix('#')?;
    let codepoint = if let Some(hex) = body.strip_prefix('x').or_else(|| body.strip_prefix('X')) {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        body.parse::<u32>().ok()?
    };

    let scalar = match codepoint {
        0x00 => 0xFFFD,
        0x80 => 0x20AC,
        0x82 => 0x201A,
        0x83 => 0x0192,
        0x84 => 0x201E,
        0x85 => 0x2026,
        0x86 => 0x2020,
        0x87 => 0x2021,
        0x88 => 0x02C6,
        0x89 => 0x2030,
        0x8A => 0x0160,
        0x8B => 0x2039,
        0x8C => 0x0152,
        0x91 => 0x2018,
        0x92 => 0x2019,
        0x93 => 0x201C,
        0x94 => 0x201D,
        0x95 => 0x2022,
        0x96 => 0x2013,
        0x97 => 0x2014,
        0x98 => 0x02DC,
        0x99 => 0x2122,
        0x9A => 0x0161,
        0x9B => 0x203A,
        0x9C => 0x0153,
        0x9F => 0x0178,
        other => other,
    };

    char::from_u32(scalar).map(|ch| ch.to_string())
}

#[cfg(test)]
mod tests {
    use super::{decode_entities, Token, Tokenizer};

    #[test]
    fn raw_text_does_not_tokenize_script_markup() {
        let tokens =
            Tokenizer::new("<script>if (a < b) { document.write('<x>'); }</script>").tokenize();
        assert!(matches!(tokens.first(), Some(Token::StartTag { name, .. }) if name == "script"));
        assert!(tokens
            .iter()
            .any(|token| matches!(token, Token::Text(text) if text.contains("<x>"))));
    }

    #[test]
    fn decodes_named_and_numeric_entities() {
        assert_eq!(
            decode_entities("&lt;&gt;&amp;&quot;&apos;&nbsp;&copy;&#65;&#x41;&mdash;"),
            "<>&\"'\u{00A0}\u{00A9}AA\u{2014}"
        );
    }

    #[test]
    fn preserves_unknown_entities() {
        assert_eq!(decode_entities("&bogus;"), "&bogus;");
    }
}
