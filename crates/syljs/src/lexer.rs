#![doc = "Lexer for the SylJS JavaScript subset."]

use crate::{
    diagnostic::{Diagnostic, SylJsError},
    Keyword, SourceId, Span, Token, TokenKind,
};

/// JavaScript subset lexer.
pub struct Lexer<'a> {
    source: &'a str,
    chars: Vec<(usize, char)>,
    index: usize,
    source_id: SourceId,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    /// Creates a lexer for source text.
    #[must_use]
    pub fn new(source: &'a str) -> Self {
        Self::with_source_id(source, SourceId::default())
    }

    /// Creates a lexer with an explicit source id.
    #[must_use]
    pub fn with_source_id(source: &'a str, source_id: SourceId) -> Self {
        Self {
            source,
            chars: source.char_indices().collect(),
            index: 0,
            source_id,
            diagnostics: Vec::new(),
        }
    }

    /// Tokenizes the full source.
    pub fn tokenize(mut self) -> Result<Vec<Token>, SylJsError> {
        let mut tokens = Vec::new();

        while !self.is_eof() {
            self.skip_trivia();

            if self.is_eof() {
                break;
            }

            let token = self.lex_token();
            tokens.push(token);
        }

        let eof = self.offset();
        tokens.push(Token::new(TokenKind::Eof, Span::point(self.source_id, eof)));

        if self.diagnostics.is_empty() {
            Ok(tokens)
        } else {
            Err(SylJsError::from_diagnostics(self.diagnostics))
        }
    }

    fn lex_token(&mut self) -> Token {
        let start = self.offset();
        let Some(ch) = self.bump() else {
            return Token::new(TokenKind::Eof, Span::point(self.source_id, start));
        };

        let kind = match ch {
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            '{' => TokenKind::LBrace,
            '}' => TokenKind::RBrace,
            '[' => TokenKind::LBracket,
            ']' => TokenKind::RBracket,
            '.' => TokenKind::Dot,
            ',' => TokenKind::Comma,
            ';' => TokenKind::Semicolon,
            ':' => TokenKind::Colon,
            '?' => TokenKind::Question,
            '~' => TokenKind::Tilde,
            '+' => {
                if self.consume_if('+') {
                    TokenKind::PlusPlus
                } else if self.consume_if('=') {
                    TokenKind::PlusEqual
                } else {
                    TokenKind::Plus
                }
            }
            '-' => {
                if self.consume_if('-') {
                    TokenKind::MinusMinus
                } else if self.consume_if('=') {
                    TokenKind::MinusEqual
                } else {
                    TokenKind::Minus
                }
            }
            '*' => {
                if self.consume_if('=') {
                    TokenKind::StarEqual
                } else {
                    TokenKind::Star
                }
            }
            '%' => {
                if self.consume_if('=') {
                    TokenKind::PercentEqual
                } else {
                    TokenKind::Percent
                }
            }
            '/' => {
                if self.consume_if('=') {
                    TokenKind::SlashEqual
                } else {
                    TokenKind::Slash
                }
            }
            '!' => {
                if self.consume_if('=') {
                    if self.consume_if('=') {
                        TokenKind::BangEqualEqual
                    } else {
                        TokenKind::BangEqual
                    }
                } else {
                    TokenKind::Bang
                }
            }
            '=' => {
                if self.consume_if('>') {
                    TokenKind::Arrow
                } else if self.consume_if('=') {
                    if self.consume_if('=') {
                        TokenKind::EqualEqualEqual
                    } else {
                        TokenKind::EqualEqual
                    }
                } else {
                    TokenKind::Equal
                }
            }
            '<' => {
                if self.consume_if('=') {
                    TokenKind::LessEqual
                } else {
                    TokenKind::Less
                }
            }
            '>' => {
                if self.consume_if('=') {
                    TokenKind::GreaterEqual
                } else {
                    TokenKind::Greater
                }
            }
            '&' => {
                if self.consume_if('&') {
                    TokenKind::AmpAmp
                } else {
                    self.error(
                        "single `&` is not supported in SylJS subset",
                        start,
                        self.offset(),
                    );
                    TokenKind::AmpAmp
                }
            }
            '|' => {
                if self.consume_if('|') {
                    TokenKind::PipePipe
                } else {
                    self.error(
                        "single `|` is not supported in SylJS subset",
                        start,
                        self.offset(),
                    );
                    TokenKind::PipePipe
                }
            }
            '"' | '\'' => return self.lex_string(start, ch),
            '`' => return self.lex_template(start),
            c if is_ident_start(c) => return self.lex_identifier(start, c),
            c if c.is_ascii_digit() => return self.lex_number(start, c),
            _ => {
                self.error(format!("unexpected character `{ch}`"), start, self.offset());
                TokenKind::Eof
            }
        };

        Token::new(kind, Span::new(self.source_id, start, self.offset()))
    }

    fn lex_identifier(&mut self, start: usize, first: char) -> Token {
        let mut value = String::new();
        value.push(first);

        while let Some(ch) = self.peek() {
            if is_ident_continue(ch) {
                value.push(ch);
                let _ = self.bump();
            } else {
                break;
            }
        }

        let kind = Keyword::from_ident(&value)
            .map(TokenKind::Keyword)
            .unwrap_or(TokenKind::Identifier(value));

        Token::new(kind, Span::new(self.source_id, start, self.offset()))
    }

    fn lex_number(&mut self, start: usize, first: char) -> Token {
        let mut value = String::new();
        value.push(first);
        let mut seen_dot = false;

        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                value.push(ch);
                let _ = self.bump();
            } else if ch == '.' && !seen_dot {
                seen_dot = true;
                value.push(ch);
                let _ = self.bump();
            } else {
                break;
            }
        }

        if matches!(self.peek(), Some('e' | 'E')) {
            value.push(self.bump().unwrap_or('e'));

            if matches!(self.peek(), Some('+' | '-')) {
                value.push(self.bump().unwrap_or('+'));
            }

            let mut digits = 0usize;
            while let Some(ch) = self.peek() {
                if ch.is_ascii_digit() {
                    digits = digits.saturating_add(1);
                    value.push(ch);
                    let _ = self.bump();
                } else {
                    break;
                }
            }

            if digits == 0 {
                self.error("expected exponent digits", start, self.offset());
            }
        }

        let number = value.parse::<f64>().unwrap_or_else(|_| {
            self.error(
                format!("invalid numeric literal `{value}`"),
                start,
                self.offset(),
            );
            0.0
        });

        Token::new(
            TokenKind::Number(number),
            Span::new(self.source_id, start, self.offset()),
        )
    }

    fn lex_string(&mut self, start: usize, quote: char) -> Token {
        let mut value = String::new();

        while let Some(ch) = self.bump() {
            if ch == quote {
                return Token::new(
                    TokenKind::String(value),
                    Span::new(self.source_id, start, self.offset()),
                );
            }

            if ch == '\\' {
                match self.bump() {
                    Some('n') => value.push('\n'),
                    Some('r') => value.push('\r'),
                    Some('t') => value.push('\t'),
                    Some('\\') => value.push('\\'),
                    Some('"') => value.push('"'),
                    Some('\'') => value.push('\''),
                    Some('0') => value.push('\0'),
                    Some(other) => value.push(other),
                    None => {
                        self.error("unterminated escape sequence", start, self.offset());
                        break;
                    }
                }
            } else {
                value.push(ch);
            }
        }

        self.error("unterminated string literal", start, self.offset());
        Token::new(
            TokenKind::String(value),
            Span::new(self.source_id, start, self.offset()),
        )
    }

    fn lex_template(&mut self, start: usize) -> Token {
        let mut value = String::new();

        while let Some(ch) = self.bump() {
            if ch == '`' {
                return Token::new(
                    TokenKind::Template(value),
                    Span::new(self.source_id, start, self.offset()),
                );
            }

            if ch == '$' && self.peek() == Some('{') {
                self.error(
                    "template interpolation is not supported in SylJS Module 28",
                    start,
                    self.offset(),
                );
            }

            value.push(ch);
        }

        self.error("unterminated template literal", start, self.offset());
        Token::new(
            TokenKind::Template(value),
            Span::new(self.source_id, start, self.offset()),
        )
    }

    fn skip_trivia(&mut self) {
        loop {
            while matches!(self.peek(), Some(ch) if ch.is_whitespace()) {
                let _ = self.bump();
            }

            if self.peek() == Some('/') && self.peek_n(1) == Some('/') {
                let _ = self.bump();
                let _ = self.bump();

                while let Some(ch) = self.peek() {
                    if ch == '\n' || ch == '\r' {
                        break;
                    }
                    let _ = self.bump();
                }

                continue;
            }

            if self.peek() == Some('/') && self.peek_n(1) == Some('*') {
                let start = self.offset();
                let _ = self.bump();
                let _ = self.bump();

                let mut closed = false;

                while let Some(ch) = self.bump() {
                    if ch == '*' && self.peek() == Some('/') {
                        let _ = self.bump();
                        closed = true;
                        break;
                    }
                }

                if !closed {
                    self.error("unterminated block comment", start, self.offset());
                }

                continue;
            }

            break;
        }
    }

    fn error(&mut self, message: impl Into<String>, start: usize, end: usize) {
        self.diagnostics.push(Diagnostic::lex(
            message,
            Span::new(self.source_id, start, end),
        ));
    }

    fn is_eof(&self) -> bool {
        self.index >= self.chars.len()
    }

    fn offset(&self) -> usize {
        self.chars
            .get(self.index)
            .map_or(self.source.len(), |(offset, _)| *offset)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.index).map(|(_, ch)| *ch)
    }

    fn peek_n(&self, n: usize) -> Option<char> {
        self.chars.get(self.index + n).map(|(_, ch)| *ch)
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.index = self.index.saturating_add(1);
        Some(ch)
    }

    fn consume_if(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            let _ = self.bump();
            true
        } else {
            false
        }
    }
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_ascii_alphabetic()
}

fn is_ident_continue(ch: char) -> bool {
    is_ident_start(ch) || ch.is_ascii_digit()
}
