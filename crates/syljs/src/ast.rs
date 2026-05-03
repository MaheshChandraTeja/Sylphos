#![doc = "AST definitions for the SylJS JavaScript subset."]

use crate::Span;
use serde::{Deserialize, Serialize};

/// Program mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProgramKind {
    /// Classic script.
    Script,

    /// ES module syntax mode.
    Module,
}

/// Parsed program.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Program {
    /// Program mode.
    pub kind: ProgramKind,

    /// Top-level statements.
    pub body: Vec<Stmt>,

    /// Whole-program span.
    pub span: Span,
}

/// Statement node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stmt {
    /// Statement kind.
    pub kind: StmtKind,

    /// Source span.
    pub span: Span,
}

/// Statement variants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StmtKind {
    /// Empty statement.
    Empty,

    /// Block statement.
    Block(Vec<Stmt>),

    /// Expression statement.
    Expr(Expr),

    /// Variable declaration.
    VarDecl(VarDecl),

    /// Function declaration.
    FunctionDecl(FunctionDecl),

    /// Return statement.
    Return(Option<Expr>),

    /// If statement.
    If {
        /// Test expression.
        test: Expr,

        /// Consequent branch.
        consequent: Box<Stmt>,

        /// Optional alternate branch.
        alternate: Option<Box<Stmt>>,
    },

    /// While loop.
    While {
        /// Test expression.
        test: Expr,

        /// Loop body.
        body: Box<Stmt>,
    },

    /// For loop.
    For {
        /// Optional init statement/expression.
        init: Option<ForInit>,

        /// Optional test expression.
        test: Option<Expr>,

        /// Optional update expression.
        update: Option<Expr>,

        /// Loop body.
        body: Box<Stmt>,
    },

    /// Break statement.
    Break,

    /// Continue statement.
    Continue,
}

/// For-loop initializer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ForInit {
    /// Variable declaration initializer.
    VarDecl(VarDecl),

    /// Expression initializer.
    Expr(Expr),
}

/// Variable declaration statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VarDecl {
    /// Declaration kind.
    pub kind: VarDeclKind,

    /// Declarators.
    pub declarations: Vec<VarDeclarator>,
}

/// Variable declaration kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VarDeclKind {
    /// `let`
    Let,
    /// `const`
    Const,
    /// `var`
    Var,
}

/// Variable declarator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VarDeclarator {
    /// Binding pattern.
    pub id: BindingPattern,

    /// Optional initializer.
    pub init: Option<Expr>,

    /// Source span.
    pub span: Span,
}

/// Binding pattern subset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BindingPattern {
    /// Identifier binding.
    Identifier(String),
}

/// Function declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionDecl {
    /// Function name.
    pub name: String,

    /// Parameters.
    pub params: Vec<FunctionParam>,

    /// Function body.
    pub body: Vec<Stmt>,

    /// Source span.
    pub span: Span,
}

/// Function parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionParam {
    /// Parameter name.
    pub name: String,

    /// Source span.
    pub span: Span,
}

/// Expression node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expr {
    /// Expression kind.
    pub kind: ExprKind,

    /// Source span.
    pub span: Span,
}

/// Expression variants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExprKind {
    /// Literal expression.
    Literal(Literal),

    /// Identifier.
    Identifier(String),

    /// `this`
    This,

    /// Array literal.
    Array(Vec<Option<Expr>>),

    /// Object literal.
    Object(Vec<ObjectProperty>),

    /// Unary expression.
    Unary {
        /// Operator.
        op: UnaryOp,

        /// Argument.
        argument: Box<Expr>,
    },

    /// Binary expression.
    Binary {
        /// Operator.
        op: BinaryOp,

        /// Left expression.
        left: Box<Expr>,

        /// Right expression.
        right: Box<Expr>,
    },

    /// Assignment expression.
    Assign {
        /// Assignment operator.
        op: AssignOp,

        /// Left expression.
        left: Box<Expr>,

        /// Right expression.
        right: Box<Expr>,
    },

    /// Member expression.
    Member {
        /// Object expression.
        object: Box<Expr>,

        /// Property expression/name.
        property: MemberProperty,
    },

    /// Call expression.
    Call {
        /// Callee expression.
        callee: Box<Expr>,

        /// Arguments.
        arguments: Vec<Expr>,
    },

    /// Function expression.
    Function {
        /// Optional function name.
        name: Option<String>,

        /// Parameters.
        params: Vec<FunctionParam>,

        /// Body.
        body: Vec<Stmt>,
    },

    /// New expression.
    New {
        /// Constructor expression.
        callee: Box<Expr>,

        /// Arguments.
        arguments: Vec<Expr>,
    },

    /// Conditional expression.
    Conditional {
        /// Test.
        test: Box<Expr>,

        /// Consequent.
        consequent: Box<Expr>,

        /// Alternate.
        alternate: Box<Expr>,
    },
}

/// Object literal property.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectProperty {
    /// Property key.
    pub key: String,

    /// Property value.
    pub value: Expr,

    /// Source span.
    pub span: Span,
}

/// Member expression property.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MemberProperty {
    /// Dot property access.
    Ident(String),

    /// Computed property access.
    Computed(Box<Expr>),
}

/// Literal value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    /// Numeric literal.
    Number(f64),

    /// String literal.
    String(String),

    /// Boolean literal.
    Boolean(bool),

    /// Null literal.
    Null,

    /// Undefined literal.
    Undefined,
}

/// Unary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOp {
    /// `!`
    Not,

    /// `-`
    Neg,

    /// `+`
    Pos,

    /// `typeof`
    Typeof,

    /// `void`
    Void,

    /// `delete`
    Delete,
}

/// Binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryOp {
    /// `+`
    Add,

    /// `-`
    Sub,

    /// `*`
    Mul,

    /// `/`
    Div,

    /// `%`
    Mod,

    /// `==`
    Eq,

    /// `!=`
    NotEq,

    /// `===`
    StrictEq,

    /// `!==`
    StrictNotEq,

    /// `<`
    Lt,

    /// `<=`
    Lte,

    /// `>`
    Gt,

    /// `>=`
    Gte,

    /// `&&`
    LogicalAnd,

    /// `||`
    LogicalOr,
}

/// Assignment operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssignOp {
    /// `=`
    Assign,

    /// `+=`
    AddAssign,

    /// `-=`
    SubAssign,

    /// `*=`
    MulAssign,

    /// `/=`
    DivAssign,

    /// `%=`
    ModAssign,
}

impl Stmt {
    /// Creates a statement.
    #[must_use]
    pub const fn new(kind: StmtKind, span: Span) -> Self {
        Self { kind, span }
    }
}

impl Expr {
    /// Creates an expression.
    #[must_use]
    pub const fn new(kind: ExprKind, span: Span) -> Self {
        Self { kind, span }
    }
}
