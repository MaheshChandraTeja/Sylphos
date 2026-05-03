#![doc = "Token definitions for the SylJS frontend."]

use crate::Span;

/// JavaScript keyword subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Keyword {
    /// `let`
    Let,
    /// `const`
    Const,
    /// `var`
    Var,
    /// `function`
    Function,
    /// `return`
    Return,
    /// `if`
    If,
    /// `else`
    Else,
    /// `while`
    While,
    /// `for`
    For,
    /// `break`
    Break,
    /// `continue`
    Continue,
    /// `true`
    True,
    /// `false`
    False,
    /// `null`
    Null,
    /// `undefined`
    Undefined,
    /// `new`
    New,
    /// `this`
    This,
    /// `typeof`
    Typeof,
    /// `void`
    Void,
    /// `delete`
    Delete,
    /// `in`
    In,
}

/// Token kind.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    /// End of file.
    Eof,

    /// Identifier.
    Identifier(String),

    /// Keyword.
    Keyword(Keyword),

    /// Numeric literal.
    Number(f64),

    /// String literal.
    String(String),

    /// Template literal without interpolation.
    Template(String),

    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `[`
    LBracket,
    /// `]`
    RBracket,

    /// `.`
    Dot,
    /// `,`
    Comma,
    /// `;`
    Semicolon,
    /// `:`
    Colon,
    /// `?`
    Question,

    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `%`
    Percent,

    /// `!`
    Bang,
    /// `~`
    Tilde,

    /// `=`
    Equal,
    /// `+=`
    PlusEqual,
    /// `-=`
    MinusEqual,
    /// `*=`
    StarEqual,
    /// `/=`
    SlashEqual,
    /// `%=`
    PercentEqual,

    /// `==`
    EqualEqual,
    /// `!=`
    BangEqual,
    /// `===`
    EqualEqualEqual,
    /// `!==`
    BangEqualEqual,

    /// `<`
    Less,
    /// `<=`
    LessEqual,
    /// `>`
    Greater,
    /// `>=`
    GreaterEqual,

    /// `&&`
    AmpAmp,
    /// `||`
    PipePipe,

    /// `++`
    PlusPlus,
    /// `--`
    MinusMinus,

    /// `=>`
    Arrow,
}

/// Token with span.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// Token kind.
    pub kind: TokenKind,

    /// Source span.
    pub span: Span,
}

impl Token {
    /// Creates a token.
    #[must_use]
    pub const fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

impl Keyword {
    /// Converts an identifier string to a keyword when applicable.
    #[must_use]
    pub fn from_ident(value: &str) -> Option<Self> {
        match value {
            "let" => Some(Self::Let),
            "const" => Some(Self::Const),
            "var" => Some(Self::Var),
            "function" => Some(Self::Function),
            "return" => Some(Self::Return),
            "if" => Some(Self::If),
            "else" => Some(Self::Else),
            "while" => Some(Self::While),
            "for" => Some(Self::For),
            "break" => Some(Self::Break),
            "continue" => Some(Self::Continue),
            "true" => Some(Self::True),
            "false" => Some(Self::False),
            "null" => Some(Self::Null),
            "undefined" => Some(Self::Undefined),
            "new" => Some(Self::New),
            "this" => Some(Self::This),
            "typeof" => Some(Self::Typeof),
            "void" => Some(Self::Void),
            "delete" => Some(Self::Delete),
            "in" => Some(Self::In),
            _ => None,
        }
    }
}
