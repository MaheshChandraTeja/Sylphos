#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Doctype {
        name: String,
    },
    StartTag {
        name: String,
        attrs: Vec<(String, String)>,
        self_closing: bool,
    },
    EndTag {
        name: String,
    },
    Comment(String),
    Text(String),
}

pub struct Tokenizer<'a> {
    input: &'a str,
}

impl<'a> Tokenizer<'a> {
    pub const fn new(input: &'a str) -> Self {
        Self { input }
    }

    pub fn tokenize(self) -> Vec<Token> {
        let chars: Vec<char> = self.input.chars().collect();
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
                    flush_text(&mut tokens, &mut text);

                    let comment = chars[index + 4..end].iter().collect::<String>();
                    tokens.push(Token::Comment(comment));
                    index = end + 3;
                    continue;
                }

                text.push('<');
                index += 1;
                continue;
            }

            if starts_with(&chars, index, "<!") {
                if let Some(end) = find_char(&chars, index + 2, '>') {
                    flush_text(&mut tokens, &mut text);

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
                    flush_text(&mut tokens, &mut text);
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
                    flush_text(&mut tokens, &mut text);
                    tokens.push(token);
                    index = next_index;
                    continue;
                }

                text.push('<');
                index += 1;
                continue;
            }

            text.push('<');
            index += 1;
        }

        flush_text(&mut tokens, &mut text);
        tokens
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

                return None;
            }
            _ => {
                let (attr, next_index) = parse_attr(chars, index)?;
                attrs.push(attr);
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

    index = skip_spaces(chars, index);

    if index < chars.len() && chars[index] == '>' {
        Some((name, index + 1))
    } else {
        None
    }
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
        return None;
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

            if index >= chars.len() {
                return None;
            }

            value = chars[value_start..index].iter().collect::<String>();
            index += 1;
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

fn flush_text(tokens: &mut Vec<Token>, text: &mut String) {
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
        return trimmed["doctype".len()..].trim().to_ascii_lowercase();
    }

    trimmed.to_ascii_lowercase()
}

fn starts_with(chars: &[char], start: usize, pattern: &str) -> bool {
    let pattern_chars: Vec<char> = pattern.chars().collect();

    if start + pattern_chars.len() > chars.len() {
        return false;
    }

    chars[start..start + pattern_chars.len()] == pattern_chars
}

fn find_sequence(chars: &[char], start: usize, pattern: &str) -> Option<usize> {
    let pattern_chars: Vec<char> = pattern.chars().collect();

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
    !is_space(c) && !matches!(c, '=' | '/' | '>')
}

pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();

    while let Some(c) = it.next() {
        if c != '&' {
            out.push(c);
            continue;
        }

        let mut name = String::new();

        while let Some(&next_char) = it.peek() {
            name.push(next_char);
            it.next();

            if next_char == ';' {
                break;
            }

            if name.len() > 10 {
                break;
            }
        }

        let decoded = decode_entity_name(&name);

        if let Some(ch) = decoded {
            out.push(ch);
        } else {
            out.push('&');
            out.push_str(&name);
        }
    }

    out
}

fn decode_entity_name(name: &str) -> Option<char> {
    match name {
        "lt;" => Some('<'),
        "gt;" => Some('>'),
        "amp;" => Some('&'),
        "quot;" => Some('"'),
        "apos;" => Some('\''),
        "nbsp;" => Some(' '),
        "copy;" => char::from_u32(0x00A9),
        "reg;" => char::from_u32(0x00AE),
        _ => decode_numeric_entity(name),
    }
}

fn decode_numeric_entity(name: &str) -> Option<char> {
    let body = name.strip_prefix('#')?.strip_suffix(';')?;

    let codepoint = if let Some(hex) = body.strip_prefix('x').or_else(|| body.strip_prefix('X')) {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        body.parse::<u32>().ok()?
    };

    char::from_u32(codepoint)
}

#[cfg(test)]
mod tests {
    use super::decode_entities;

    #[test]
    fn decodes_named_and_numeric_entities() {
        assert_eq!(
            decode_entities("&lt;&gt;&amp;&quot;&apos;&nbsp;&copy;&#65;&#x41;"),
            "<>&\"' \u{00A9}AA"
        );
    }

    #[test]
    fn preserves_unknown_entities() {
        assert_eq!(decode_entities("&bogus;"), "&bogus;");
    }
}
