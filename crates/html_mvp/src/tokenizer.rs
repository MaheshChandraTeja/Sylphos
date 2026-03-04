use thiserror::Error;

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

#[derive(Debug, Error)]
pub enum TokenizeError {
    #[error("unexpected end of input")]
    Eof,
    #[error("malformed tag")]
    MalformedTag,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum State {
    Data,
    TagOpen,
    EndTagOpen,
    TagName,
    BeforeAttrName,
    AttrName,
    AfterAttrName,
    BeforeAttrValue,
    AttrValueUnquoted,
    AttrValueSingleQuoted,
    AttrValueDoubleQuoted,
    SelfClosingStartTag,
    CommentStart,
    CommentStartDash,
    Comment,
    CommentEndDash,
    CommentEnd,
    DoctypeStart,
    DoctypeName,
}

pub struct Tokenizer<'a> {
    it: std::str::Chars<'a>,
    _buf: String,
    cur: Option<char>,
}

impl<'a> Tokenizer<'a> {
    pub fn new(input: &'a str) -> Self {
        let mut it = input.chars();
        let cur = it.next();
        Self {
            it,
            _buf: String::new(),
            cur,
        }
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.cur;
        self.cur = self.it.next();
        c
    }

    fn peek(&self) -> Option<char> {
        self.cur
    }

    fn push(&mut self, s: &mut String, c: char) {
        s.push(c);
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, TokenizeError> {
        use State::*;
        let mut out = Vec::new();
        let mut state = Data;

        let mut tag_name = String::new();
        let mut attrs: Vec<(String, String)> = Vec::new();
        let mut attr_name = String::new();
        let mut attr_val = String::new();
        let mut self_closing = false;

        let mut text_buf = String::new();
        let mut comment_buf = String::new();
        let mut doctype_name = String::new();

        while let Some(c) = self.bump() {
            match state {
                Data => match c {
                    '<' => {
                        if !text_buf.is_empty() {
                            out.push(Token::Text(decode_entities(&text_buf)));
                            text_buf.clear();
                        }
                        state = TagOpen;
                    }
                    _ => self.push(&mut text_buf, c),
                },
                TagOpen => match c {
                    '/' => {
                        state = EndTagOpen;
                    }
                    '!' => {
                        if self.peek() == Some('-') {
                            self.bump();
                            if self.peek() == Some('-') {
                                self.bump();
                                comment_buf.clear();
                                state = CommentStart;
                            } else {
                                return Err(TokenizeError::MalformedTag);
                            }
                        } else {
                            state = DoctypeStart;
                        }
                    }
                    c if is_ascii_alpha(c) => {
                        tag_name.clear();
                        attrs.clear();
                        attr_name.clear();
                        attr_val.clear();
                        self_closing = false;
                        tag_name.push(c.to_ascii_lowercase());
                        state = TagName;
                    }
                    _ => return Err(TokenizeError::MalformedTag),
                },
                EndTagOpen => {
                    if is_ascii_alpha(c) {
                        tag_name.clear();
                        tag_name.push(c.to_ascii_lowercase());
                        state = TagName;
                    } else {
                        return Err(TokenizeError::MalformedTag);
                    }
                }
                TagName => match c {
                    c if is_space(c) => state = BeforeAttrName,
                    '/' => state = SelfClosingStartTag,
                    '>' => {
                        if self.buf_is_end_tag(&out) {
                            out.push(Token::EndTag {
                                name: tag_name.clone(),
                            });
                        } else {
                            out.push(Token::StartTag {
                                name: tag_name.clone(),
                                attrs: attrs.drain(..).collect(),
                                self_closing,
                            });
                        }
                        state = Data;
                    }
                    c => tag_name.push(c.to_ascii_lowercase()),
                },
                BeforeAttrName => match c {
                    c if is_space(c) => {}
                    '/' => state = SelfClosingStartTag,
                    '>' => {
                        if self.buf_is_end_tag(&out) {
                            out.push(Token::EndTag {
                                name: tag_name.clone(),
                            });
                        } else {
                            out.push(Token::StartTag {
                                name: tag_name.clone(),
                                attrs: attrs.drain(..).collect(),
                                self_closing,
                            });
                        }
                        state = Data;
                    }
                    c => {
                        attr_name.clear();
                        attr_val.clear();
                        attr_name.push(c.to_ascii_lowercase());
                        state = AttrName;
                    }
                },
                AttrName => match c {
                    c if is_space(c) => state = AfterAttrName,
                    '=' => state = BeforeAttrValue,
                    '/' => {
                        attrs.push((attr_name.clone(), String::new()));
                        state = SelfClosingStartTag;
                    }
                    '>' => {
                        attrs.push((attr_name.clone(), String::new()));
                        if self.buf_is_end_tag(&out) {
                            out.push(Token::EndTag {
                                name: tag_name.clone(),
                            });
                        } else {
                            out.push(Token::StartTag {
                                name: tag_name.clone(),
                                attrs: attrs.drain(..).collect(),
                                self_closing,
                            });
                        }
                        state = Data;
                    }
                    c => attr_name.push(c.to_ascii_lowercase()),
                },
                AfterAttrName => match c {
                    c if is_space(c) => {}
                    '=' => state = BeforeAttrValue,
                    '/' => {
                        attrs.push((attr_name.clone(), String::new()));
                        state = SelfClosingStartTag;
                    }
                    '>' => {
                        attrs.push((attr_name.clone(), String::new()));
                        if self.buf_is_end_tag(&out) {
                            out.push(Token::EndTag {
                                name: tag_name.clone(),
                            });
                        } else {
                            out.push(Token::StartTag {
                                name: tag_name.clone(),
                                attrs: attrs.drain(..).collect(),
                                self_closing,
                            });
                        }
                        state = Data;
                    }
                    c => {
                        attrs.push((attr_name.clone(), String::new()));
                        attr_name.clear();
                        attr_val.clear();
                        attr_name.push(c.to_ascii_lowercase());
                        state = AttrName;
                    }
                },
                BeforeAttrValue => match c {
                    c if is_space(c) => {}
                    '"' => {
                        attr_val.clear();
                        state = AttrValueDoubleQuoted;
                    }
                    '\'' => {
                        attr_val.clear();
                        state = AttrValueSingleQuoted;
                    }
                    '>' => {
                        attrs.push((attr_name.clone(), String::new()));
                        if self.buf_is_end_tag(&out) {
                            out.push(Token::EndTag {
                                name: tag_name.clone(),
                            });
                        } else {
                            out.push(Token::StartTag {
                                name: tag_name.clone(),
                                attrs: attrs.drain(..).collect(),
                                self_closing,
                            });
                        }
                        state = Data;
                    }
                    c => {
                        attr_val.clear();
                        attr_val.push(c);
                        state = AttrValueUnquoted;
                    }
                },
                AttrValueUnquoted => match c {
                    c if is_space(c) => {
                        attrs.push((attr_name.clone(), decode_entities(&attr_val)));
                        state = BeforeAttrName;
                    }
                    '>' => {
                        attrs.push((attr_name.clone(), decode_entities(&attr_val)));
                        if self.buf_is_end_tag(&out) {
                            out.push(Token::EndTag {
                                name: tag_name.clone(),
                            });
                        } else {
                            out.push(Token::StartTag {
                                name: tag_name.clone(),
                                attrs: attrs.drain(..).collect(),
                                self_closing,
                            });
                        }
                        state = Data;
                    }
                    c => attr_val.push(c),
                },
                AttrValueSingleQuoted => match c {
                    '\'' => {
                        attrs.push((attr_name.clone(), decode_entities(&attr_val)));
                        state = BeforeAttrName;
                    }
                    c => attr_val.push(c),
                },
                AttrValueDoubleQuoted => match c {
                    '"' => {
                        attrs.push((attr_name.clone(), decode_entities(&attr_val)));
                        state = BeforeAttrName;
                    }
                    c => attr_val.push(c),
                },
                SelfClosingStartTag => match c {
                    '>' => {
                        self_closing = true;
                        out.push(Token::StartTag {
                            name: tag_name.clone(),
                            attrs: attrs.drain(..).collect(),
                            self_closing,
                        });
                        self_closing = false;
                        state = Data;
                    }
                    c if is_space(c) => {}
                    _ => return Err(TokenizeError::MalformedTag),
                },
                CommentStart => match c {
                    '-' => state = CommentStartDash,
                    _ => {
                        comment_buf.push(c);
                        state = Comment;
                    }
                },
                CommentStartDash => match c {
                    '-' => state = CommentEnd,
                    _ => {
                        comment_buf.push('-');
                        comment_buf.push(c);
                        state = Comment;
                    }
                },
                Comment => match c {
                    '-' => state = CommentEndDash,
                    c => comment_buf.push(c),
                },
                CommentEndDash => match c {
                    '-' => state = CommentEnd,
                    c => {
                        comment_buf.push('-');
                        comment_buf.push(c);
                        state = Comment;
                    }
                },
                CommentEnd => match c {
                    '>' => {
                        out.push(Token::Comment(comment_buf.clone()));
                        comment_buf.clear();
                        state = Data;
                    }
                    '-' => {}
                    c => {
                        comment_buf.push_str("--");
                        comment_buf.push(c);
                        state = Comment;
                    }
                },
                DoctypeStart => match c {
                    c if is_space(c) => {}
                    _ => {
                        doctype_name.clear();
                        doctype_name.push(c);
                        state = DoctypeName;
                    }
                },
                DoctypeName => match c {
                    '>' => {
                        out.push(Token::Doctype {
                            name: doctype_name.trim().to_string(),
                        });
                        state = Data;
                    }
                    c => doctype_name.push(c),
                },
            }
        }

        if !matches!(state, State::Data) {
            return Err(TokenizeError::Eof);
        }
        if !text_buf.is_empty() {
            out.push(Token::Text(decode_entities(&text_buf)));
        }
        Ok(out)
    }

    fn buf_is_end_tag(&self, out: &[Token]) -> bool {
        let _ = out;
        false
    }
}

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\n' | '\t' | '\r' | '\x0C')
}
fn is_ascii_alpha(c: char) -> bool {
    c.is_ascii_alphabetic()
}

pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '&' {
            let mut name = String::new();
            while let Some(&nc) = it.peek() {
                name.push(nc);
                it.next();
                if nc == ';' {
                    break;
                }
                if name.len() > 10 {
                    break;
                }
            }
            let decoded = match name.as_str() {
                "lt;" => Some('<'),
                "gt;" => Some('>'),
                "amp;" => Some('&'),
                "quot;" => Some('"'),
                "apos;" => Some('\''),
                _ => None,
            };
            if let Some(ch) = decoded {
                out.push(ch);
            } else {
                out.push('&');
                out.push_str(&name);
            }
        } else {
            out.push(c);
        }
    }
    out
}
