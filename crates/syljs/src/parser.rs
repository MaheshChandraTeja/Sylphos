#![doc = "Recursive-descent / Pratt parser for the SylJS subset."]

use crate::{
    ast::{
        AssignOp, BinaryOp, BindingPattern, Expr, ExprKind, ForInit, FunctionDecl, FunctionParam,
        Literal, MemberProperty, ObjectProperty, Program, ProgramKind, Stmt, StmtKind, UnaryOp,
        VarDecl, VarDeclKind, VarDeclarator,
    },
    diagnostic::{Diagnostic, SylJsError},
    lexer::Lexer,
    Keyword, SourceId, Span, Token, TokenKind,
};

/// Parses source as a classic script.
pub fn parse_script(source: &str) -> Result<Program, SylJsError> {
    Parser::new(source, ProgramKind::Script)?.parse_program()
}

/// Parses source as a module.
pub fn parse_module(source: &str) -> Result<Program, SylJsError> {
    Parser::new(source, ProgramKind::Module)?.parse_program()
}

/// SylJS parser.
pub struct Parser {
    tokens: Vec<Token>,
    index: usize,
    source_id: SourceId,
    kind: ProgramKind,
    diagnostics: Vec<Diagnostic>,
}

impl Parser {
    /// Creates a parser from source.
    pub fn new(source: &str, kind: ProgramKind) -> Result<Self, SylJsError> {
        let tokens = Lexer::new(source).tokenize()?;

        Ok(Self {
            tokens,
            index: 0,
            source_id: SourceId::default(),
            kind,
            diagnostics: Vec::new(),
        })
    }

    /// Parses the program.
    pub fn parse_program(mut self) -> Result<Program, SylJsError> {
        let start = self.peek().span.start;
        let mut body = Vec::new();

        while !self.at_eof() {
            match self.parse_statement() {
                Ok(stmt) => body.push(stmt),
                Err(diagnostic) => {
                    self.diagnostics.push(diagnostic);
                    self.synchronize();
                }
            }
        }

        let end = self.peek().span.end;
        let program = Program {
            kind: self.kind,
            body,
            span: Span::new(self.source_id, start, end),
        };

        if self.diagnostics.is_empty() {
            Ok(program)
        } else {
            Err(SylJsError::from_diagnostics(self.diagnostics))
        }
    }

    fn parse_statement(&mut self) -> Result<Stmt, Diagnostic> {
        match &self.peek().kind {
            TokenKind::Semicolon => {
                let token = self.bump();
                Ok(Stmt::new(StmtKind::Empty, token.span))
            }
            TokenKind::LBrace => self.parse_block_statement(),
            TokenKind::Keyword(Keyword::Let | Keyword::Const | Keyword::Var) => {
                self.parse_var_decl_statement(true)
            }
            TokenKind::Keyword(Keyword::Function) => self.parse_function_decl_statement(),
            TokenKind::Keyword(Keyword::Return) => self.parse_return_statement(),
            TokenKind::Keyword(Keyword::If) => self.parse_if_statement(),
            TokenKind::Keyword(Keyword::While) => self.parse_while_statement(),
            TokenKind::Keyword(Keyword::For) => self.parse_for_statement(),
            TokenKind::Keyword(Keyword::Break) => {
                let token = self.bump();
                self.consume_semicolon();
                Ok(Stmt::new(StmtKind::Break, token.span))
            }
            TokenKind::Keyword(Keyword::Continue) => {
                let token = self.bump();
                self.consume_semicolon();
                Ok(Stmt::new(StmtKind::Continue, token.span))
            }
            _ => self.parse_expr_statement(),
        }
    }

    fn parse_block_statement(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.expect_kind(TokenKind::LBrace, "expected `{`")?.span;
        let mut body = Vec::new();

        while !self.at_eof() && !matches!(self.peek().kind, TokenKind::RBrace) {
            body.push(self.parse_statement()?);
        }

        let end = self.expect_kind(TokenKind::RBrace, "expected `}`")?.span;
        Ok(Stmt::new(StmtKind::Block(body), start.join(end)))
    }

    fn parse_var_decl_statement(&mut self, require_semicolon: bool) -> Result<Stmt, Diagnostic> {
        let start = self.peek().span;
        let decl = self.parse_var_decl()?;
        let end = if require_semicolon {
            self.consume_semicolon().unwrap_or(start)
        } else {
            self.previous_span()
        };
        Ok(Stmt::new(StmtKind::VarDecl(decl), start.join(end)))
    }

    fn parse_var_decl(&mut self) -> Result<VarDecl, Diagnostic> {
        let kind = match self.bump().kind {
            TokenKind::Keyword(Keyword::Let) => VarDeclKind::Let,
            TokenKind::Keyword(Keyword::Const) => VarDeclKind::Const,
            TokenKind::Keyword(Keyword::Var) => VarDeclKind::Var,
            _ => return Err(self.error_here("expected variable declaration kind")),
        };

        let mut declarations = Vec::new();

        loop {
            let name_token = self.expect_identifier("expected variable name")?;
            let id = match name_token.kind {
                TokenKind::Identifier(name) => BindingPattern::Identifier(name),
                _ => return Err(Diagnostic::parse("expected identifier", name_token.span)),
            };

            let init = if self.consume_kind(&TokenKind::Equal).is_some() {
                Some(self.parse_expression()?)
            } else {
                None
            };

            let span = name_token
                .span
                .join(init.as_ref().map_or(name_token.span, |expr| expr.span));
            declarations.push(VarDeclarator { id, init, span });

            if self.consume_kind(&TokenKind::Comma).is_none() {
                break;
            }
        }

        Ok(VarDecl { kind, declarations })
    }

    fn parse_function_decl_statement(&mut self) -> Result<Stmt, Diagnostic> {
        let function = self.parse_function_decl(true)?;
        Ok(Stmt::new(
            StmtKind::FunctionDecl(function.clone()),
            function.span,
        ))
    }

    fn parse_function_decl(&mut self, require_name: bool) -> Result<FunctionDecl, Diagnostic> {
        let start = self
            .expect_keyword(Keyword::Function, "expected `function`")?
            .span;

        let name = if matches!(self.peek().kind, TokenKind::Identifier(_)) {
            match self.bump().kind {
                TokenKind::Identifier(name) => name,
                _ => String::new(),
            }
        } else if require_name {
            return Err(self.error_here("expected function name"));
        } else {
            String::new()
        };

        let params = self.parse_params()?;
        let body_stmt = self.parse_block_statement()?;

        let StmtKind::Block(body) = body_stmt.kind else {
            return Err(Diagnostic::parse("expected function body", body_stmt.span));
        };

        let span = start.join(body_stmt.span);

        Ok(FunctionDecl {
            name,
            params,
            body,
            span,
        })
    }

    fn parse_params(&mut self) -> Result<Vec<FunctionParam>, Diagnostic> {
        self.expect_kind(TokenKind::LParen, "expected `(` before parameters")?;
        let mut params = Vec::new();

        if self.consume_kind(&TokenKind::RParen).is_some() {
            return Ok(params);
        }

        loop {
            let token = self.expect_identifier("expected parameter name")?;
            let TokenKind::Identifier(name) = token.kind else {
                return Err(Diagnostic::parse(
                    "expected parameter identifier",
                    token.span,
                ));
            };
            params.push(FunctionParam {
                name,
                span: token.span,
            });

            if self.consume_kind(&TokenKind::Comma).is_some() {
                continue;
            }

            self.expect_kind(TokenKind::RParen, "expected `)` after parameters")?;
            break;
        }

        Ok(params)
    }

    fn parse_return_statement(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self
            .expect_keyword(Keyword::Return, "expected `return`")?
            .span;

        let argument = if self.at_eof()
            || matches!(self.peek().kind, TokenKind::Semicolon | TokenKind::RBrace)
        {
            None
        } else {
            Some(self.parse_expression()?)
        };

        let end = self
            .consume_semicolon()
            .unwrap_or_else(|| argument.as_ref().map_or(start, |expr| expr.span));

        Ok(Stmt::new(StmtKind::Return(argument), start.join(end)))
    }

    fn parse_if_statement(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.expect_keyword(Keyword::If, "expected `if`")?.span;
        self.expect_kind(TokenKind::LParen, "expected `(` after if")?;
        let test = self.parse_expression()?;
        self.expect_kind(TokenKind::RParen, "expected `)` after if test")?;
        let consequent = Box::new(self.parse_statement()?);
        let alternate = if self.consume_keyword(Keyword::Else).is_some() {
            Some(Box::new(self.parse_statement()?))
        } else {
            None
        };

        let end = alternate.as_ref().map_or(consequent.span, |stmt| stmt.span);

        Ok(Stmt::new(
            StmtKind::If {
                test,
                consequent,
                alternate,
            },
            start.join(end),
        ))
    }

    fn parse_while_statement(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self
            .expect_keyword(Keyword::While, "expected `while`")?
            .span;
        self.expect_kind(TokenKind::LParen, "expected `(` after while")?;
        let test = self.parse_expression()?;
        self.expect_kind(TokenKind::RParen, "expected `)` after while test")?;
        let body = Box::new(self.parse_statement()?);
        Ok(Stmt::new(
            StmtKind::While {
                test,
                body: body.clone(),
            },
            start.join(body.span),
        ))
    }

    fn parse_for_statement(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.expect_keyword(Keyword::For, "expected `for`")?.span;
        self.expect_kind(TokenKind::LParen, "expected `(` after for")?;

        let init = if self.consume_kind(&TokenKind::Semicolon).is_some() {
            None
        } else if matches!(
            self.peek().kind,
            TokenKind::Keyword(Keyword::Let | Keyword::Const | Keyword::Var)
        ) {
            let decl = self.parse_var_decl()?;
            self.expect_kind(TokenKind::Semicolon, "expected `;` after for initializer")?;
            Some(ForInit::VarDecl(decl))
        } else {
            let expr = self.parse_expression()?;
            self.expect_kind(TokenKind::Semicolon, "expected `;` after for initializer")?;
            Some(ForInit::Expr(expr))
        };

        let test = if self.consume_kind(&TokenKind::Semicolon).is_some() {
            None
        } else {
            let expr = self.parse_expression()?;
            self.expect_kind(TokenKind::Semicolon, "expected `;` after for test")?;
            Some(expr)
        };

        let update = if self.consume_kind(&TokenKind::RParen).is_some() {
            None
        } else {
            let expr = self.parse_expression()?;
            self.expect_kind(TokenKind::RParen, "expected `)` after for update")?;
            Some(expr)
        };

        let body = Box::new(self.parse_statement()?);

        Ok(Stmt::new(
            StmtKind::For {
                init,
                test,
                update,
                body: body.clone(),
            },
            start.join(body.span),
        ))
    }

    fn parse_expr_statement(&mut self) -> Result<Stmt, Diagnostic> {
        let expr = self.parse_expression()?;
        let span = expr
            .span
            .join(self.consume_semicolon().unwrap_or(expr.span));
        Ok(Stmt::new(StmtKind::Expr(expr), span))
    }

    fn parse_expression(&mut self) -> Result<Expr, Diagnostic> {
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> Result<Expr, Diagnostic> {
        let left = self.parse_conditional()?;

        let op = match self.peek().kind {
            TokenKind::Equal => AssignOp::Assign,
            TokenKind::PlusEqual => AssignOp::AddAssign,
            TokenKind::MinusEqual => AssignOp::SubAssign,
            TokenKind::StarEqual => AssignOp::MulAssign,
            TokenKind::SlashEqual => AssignOp::DivAssign,
            TokenKind::PercentEqual => AssignOp::ModAssign,
            _ => return Ok(left),
        };

        let _op_token = self.bump();
        let right = self.parse_assignment()?;
        let span = left.span.join(right.span);

        Ok(Expr::new(
            ExprKind::Assign {
                op,
                left: Box::new(left),
                right: Box::new(right),
            },
            span,
        ))
    }

    fn parse_conditional(&mut self) -> Result<Expr, Diagnostic> {
        let test = self.parse_binary(0)?;

        if self.consume_kind(&TokenKind::Question).is_none() {
            return Ok(test);
        }

        let consequent = self.parse_expression()?;
        self.expect_kind(TokenKind::Colon, "expected `:` in conditional expression")?;
        let alternate = self.parse_expression()?;
        let span = test.span.join(alternate.span);

        Ok(Expr::new(
            ExprKind::Conditional {
                test: Box::new(test),
                consequent: Box::new(consequent),
                alternate: Box::new(alternate),
            },
            span,
        ))
    }

    fn parse_binary(&mut self, min_prec: u8) -> Result<Expr, Diagnostic> {
        let mut left = self.parse_unary()?;

        while let Some((op, prec)) = self.current_binary_op() {
            if prec < min_prec {
                break;
            }

            let _ = self.bump();
            let right = self.parse_binary(prec.saturating_add(1))?;
            let span = left.span.join(right.span);
            left = Expr::new(
                ExprKind::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
        }

        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, Diagnostic> {
        let op = match self.peek().kind {
            TokenKind::Bang => Some(UnaryOp::Not),
            TokenKind::Minus => Some(UnaryOp::Neg),
            TokenKind::Plus => Some(UnaryOp::Pos),
            TokenKind::Keyword(Keyword::Typeof) => Some(UnaryOp::Typeof),
            TokenKind::Keyword(Keyword::Void) => Some(UnaryOp::Void),
            TokenKind::Keyword(Keyword::Delete) => Some(UnaryOp::Delete),
            _ => None,
        };

        if let Some(op) = op {
            let token = self.bump();
            let argument = self.parse_unary()?;
            let span = token.span.join(argument.span);
            return Ok(Expr::new(
                ExprKind::Unary {
                    op,
                    argument: Box::new(argument),
                },
                span,
            ));
        }

        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, Diagnostic> {
        let mut expr = self.parse_primary()?;

        loop {
            match self.peek().kind.clone() {
                TokenKind::Dot => {
                    let _ = self.bump();
                    let property_token =
                        self.expect_identifier("expected property name after `.`")?;
                    let TokenKind::Identifier(name) = property_token.kind else {
                        return Err(Diagnostic::parse(
                            "expected property name",
                            property_token.span,
                        ));
                    };
                    let span = expr.span.join(property_token.span);
                    expr = Expr::new(
                        ExprKind::Member {
                            object: Box::new(expr),
                            property: MemberProperty::Ident(name),
                        },
                        span,
                    );
                }
                TokenKind::LBracket => {
                    let start = self.bump().span;
                    let property = self.parse_expression()?;
                    let end = self.expect_kind(TokenKind::RBracket, "expected `]`")?.span;
                    let span = expr.span.join(end);
                    let _ = start;
                    expr = Expr::new(
                        ExprKind::Member {
                            object: Box::new(expr),
                            property: MemberProperty::Computed(Box::new(property)),
                        },
                        span,
                    );
                }
                TokenKind::LParen => {
                    let args = self.parse_arguments()?;
                    let end = args.last().map_or(self.previous_span(), |arg| arg.span);
                    let span = expr.span.join(end);
                    expr = Expr::new(
                        ExprKind::Call {
                            callee: Box::new(expr),
                            arguments: args,
                        },
                        span,
                    );
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    fn parse_arguments(&mut self) -> Result<Vec<Expr>, Diagnostic> {
        self.expect_kind(TokenKind::LParen, "expected `(`")?;
        let mut args = Vec::new();

        if self.consume_kind(&TokenKind::RParen).is_some() {
            return Ok(args);
        }

        loop {
            args.push(self.parse_expression()?);

            if self.consume_kind(&TokenKind::Comma).is_some() {
                if matches!(self.peek().kind, TokenKind::RParen) {
                    break;
                }
                continue;
            }

            break;
        }

        self.expect_kind(TokenKind::RParen, "expected `)` after arguments")?;
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<Expr, Diagnostic> {
        let token = self.bump();

        match token.kind {
            TokenKind::Number(value) => Ok(Expr::new(
                ExprKind::Literal(Literal::Number(value)),
                token.span,
            )),
            TokenKind::String(value) | TokenKind::Template(value) => Ok(Expr::new(
                ExprKind::Literal(Literal::String(value)),
                token.span,
            )),
            TokenKind::Identifier(name) => Ok(Expr::new(ExprKind::Identifier(name), token.span)),
            TokenKind::Keyword(Keyword::True) => Ok(Expr::new(
                ExprKind::Literal(Literal::Boolean(true)),
                token.span,
            )),
            TokenKind::Keyword(Keyword::False) => Ok(Expr::new(
                ExprKind::Literal(Literal::Boolean(false)),
                token.span,
            )),
            TokenKind::Keyword(Keyword::Null) => {
                Ok(Expr::new(ExprKind::Literal(Literal::Null), token.span))
            }
            TokenKind::Keyword(Keyword::Undefined) => {
                Ok(Expr::new(ExprKind::Literal(Literal::Undefined), token.span))
            }
            TokenKind::Keyword(Keyword::This) => Ok(Expr::new(ExprKind::This, token.span)),
            TokenKind::Keyword(Keyword::Function) => self.parse_function_expression(token.span),
            TokenKind::Keyword(Keyword::New) => self.parse_new_expression(token.span),
            TokenKind::LParen => {
                let expr = self.parse_expression()?;
                self.expect_kind(TokenKind::RParen, "expected `)`")?;
                Ok(expr)
            }
            TokenKind::LBracket => self.parse_array_literal(token.span),
            TokenKind::LBrace => self.parse_object_literal(token.span),
            _ => Err(Diagnostic::parse("expected expression", token.span)),
        }
    }

    fn parse_function_expression(&mut self, start: Span) -> Result<Expr, Diagnostic> {
        let name = if matches!(self.peek().kind, TokenKind::Identifier(_)) {
            match self.bump().kind {
                TokenKind::Identifier(name) => Some(name),
                _ => None,
            }
        } else {
            None
        };

        let params = self.parse_params()?;
        let body_stmt = self.parse_block_statement()?;

        let StmtKind::Block(body) = body_stmt.kind else {
            return Err(Diagnostic::parse(
                "expected function expression body",
                body_stmt.span,
            ));
        };

        let span = start.join(body_stmt.span);

        Ok(Expr::new(ExprKind::Function { name, params, body }, span))
    }

    fn parse_new_expression(&mut self, start: Span) -> Result<Expr, Diagnostic> {
        let callee = self.parse_member_expression()?;
        let arguments = if matches!(self.peek().kind, TokenKind::LParen) {
            self.parse_arguments()?
        } else {
            Vec::new()
        };

        let end = arguments
            .last()
            .map_or(callee.span, |arg| arg.span)
            .join(callee.span);

        Ok(Expr::new(
            ExprKind::New {
                callee: Box::new(callee),
                arguments,
            },
            start.join(end),
        ))
    }

    fn parse_member_expression(&mut self) -> Result<Expr, Diagnostic> {
        let mut expr = self.parse_primary()?;

        loop {
            match self.peek().kind.clone() {
                TokenKind::Dot => {
                    let _ = self.bump();
                    let property_token =
                        self.expect_identifier("expected property name after `.`")?;
                    let TokenKind::Identifier(name) = property_token.kind else {
                        return Err(Diagnostic::parse(
                            "expected property name",
                            property_token.span,
                        ));
                    };
                    let span = expr.span.join(property_token.span);
                    expr = Expr::new(
                        ExprKind::Member {
                            object: Box::new(expr),
                            property: MemberProperty::Ident(name),
                        },
                        span,
                    );
                }
                TokenKind::LBracket => {
                    let start = self.bump().span;
                    let property = self.parse_expression()?;
                    let end = self.expect_kind(TokenKind::RBracket, "expected `]`")?.span;
                    let span = expr.span.join(end);
                    let _ = start;
                    expr = Expr::new(
                        ExprKind::Member {
                            object: Box::new(expr),
                            property: MemberProperty::Computed(Box::new(property)),
                        },
                        span,
                    );
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    fn parse_array_literal(&mut self, start: Span) -> Result<Expr, Diagnostic> {
        let mut items = Vec::new();

        if self.consume_kind(&TokenKind::RBracket).is_some() {
            return Ok(Expr::new(ExprKind::Array(items), start));
        }

        loop {
            if self.consume_kind(&TokenKind::Comma).is_some() {
                items.push(None);
                continue;
            }

            items.push(Some(self.parse_expression()?));

            if self.consume_kind(&TokenKind::Comma).is_some() {
                if matches!(self.peek().kind, TokenKind::RBracket) {
                    break;
                }
                continue;
            }

            break;
        }

        let end = self.expect_kind(TokenKind::RBracket, "expected `]`")?.span;
        Ok(Expr::new(ExprKind::Array(items), start.join(end)))
    }

    fn parse_object_literal(&mut self, start: Span) -> Result<Expr, Diagnostic> {
        let mut properties = Vec::new();

        if self.consume_kind(&TokenKind::RBrace).is_some() {
            return Ok(Expr::new(ExprKind::Object(properties), start));
        }

        loop {
            let key_token = self.bump();
            let key = match key_token.kind {
                TokenKind::Identifier(name) | TokenKind::String(name) => name,
                TokenKind::Number(value) => value.to_string(),
                _ => {
                    return Err(Diagnostic::parse(
                        "expected object property key",
                        key_token.span,
                    ))
                }
            };

            let value = if self.consume_kind(&TokenKind::Colon).is_some() {
                self.parse_expression()?
            } else {
                Expr::new(ExprKind::Identifier(key.clone()), key_token.span)
            };

            let span = key_token.span.join(value.span);
            properties.push(ObjectProperty { key, value, span });

            if self.consume_kind(&TokenKind::Comma).is_some() {
                if matches!(self.peek().kind, TokenKind::RBrace) {
                    break;
                }
                continue;
            }

            break;
        }

        let end = self.expect_kind(TokenKind::RBrace, "expected `}`")?.span;
        Ok(Expr::new(ExprKind::Object(properties), start.join(end)))
    }

    fn current_binary_op(&self) -> Option<(BinaryOp, u8)> {
        match self.peek().kind {
            TokenKind::PipePipe => Some((BinaryOp::LogicalOr, 1)),
            TokenKind::AmpAmp => Some((BinaryOp::LogicalAnd, 2)),
            TokenKind::EqualEqual => Some((BinaryOp::Eq, 3)),
            TokenKind::BangEqual => Some((BinaryOp::NotEq, 3)),
            TokenKind::EqualEqualEqual => Some((BinaryOp::StrictEq, 3)),
            TokenKind::BangEqualEqual => Some((BinaryOp::StrictNotEq, 3)),
            TokenKind::Less => Some((BinaryOp::Lt, 4)),
            TokenKind::LessEqual => Some((BinaryOp::Lte, 4)),
            TokenKind::Greater => Some((BinaryOp::Gt, 4)),
            TokenKind::GreaterEqual => Some((BinaryOp::Gte, 4)),
            TokenKind::Plus => Some((BinaryOp::Add, 5)),
            TokenKind::Minus => Some((BinaryOp::Sub, 5)),
            TokenKind::Star => Some((BinaryOp::Mul, 6)),
            TokenKind::Slash => Some((BinaryOp::Div, 6)),
            TokenKind::Percent => Some((BinaryOp::Mod, 6)),
            _ => None,
        }
    }

    fn expect_identifier(&mut self, message: &'static str) -> Result<Token, Diagnostic> {
        if matches!(self.peek().kind, TokenKind::Identifier(_)) {
            Ok(self.bump())
        } else {
            Err(self.error_here(message))
        }
    }

    fn expect_keyword(
        &mut self,
        keyword: Keyword,
        message: &'static str,
    ) -> Result<Token, Diagnostic> {
        if matches!(self.peek().kind, TokenKind::Keyword(found) if found == keyword) {
            Ok(self.bump())
        } else {
            Err(self.error_here(message))
        }
    }

    fn expect_kind(
        &mut self,
        expected: TokenKind,
        message: &'static str,
    ) -> Result<Token, Diagnostic> {
        if token_kind_eq(&self.peek().kind, &expected) {
            Ok(self.bump())
        } else {
            Err(self.error_here(message))
        }
    }

    fn consume_keyword(&mut self, keyword: Keyword) -> Option<Token> {
        if matches!(self.peek().kind, TokenKind::Keyword(found) if found == keyword) {
            Some(self.bump())
        } else {
            None
        }
    }

    fn consume_kind(&mut self, expected: &TokenKind) -> Option<Token> {
        if token_kind_eq(&self.peek().kind, expected) {
            Some(self.bump())
        } else {
            None
        }
    }

    fn consume_semicolon(&mut self) -> Option<Span> {
        self.consume_kind(&TokenKind::Semicolon)
            .map(|token| token.span)
    }

    fn synchronize(&mut self) {
        while !self.at_eof() {
            if matches!(self.peek().kind, TokenKind::Semicolon) {
                let _ = self.bump();
                return;
            }

            if matches!(
                self.peek().kind,
                TokenKind::Keyword(
                    Keyword::Let
                        | Keyword::Const
                        | Keyword::Var
                        | Keyword::Function
                        | Keyword::If
                        | Keyword::For
                        | Keyword::While
                        | Keyword::Return
                )
            ) {
                return;
            }

            let _ = self.bump();
        }
    }

    fn error_here(&self, message: impl Into<String>) -> Diagnostic {
        Diagnostic::parse(message, self.peek().span)
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    fn peek(&self) -> &Token {
        self.tokens
            .get(self.index)
            .unwrap_or_else(|| self.tokens.last().expect("token stream always has EOF"))
    }

    fn bump(&mut self) -> Token {
        let token = self.peek().clone();

        if !matches!(token.kind, TokenKind::Eof) {
            self.index = self.index.saturating_add(1);
        }

        token
    }

    fn previous_span(&self) -> Span {
        self.tokens
            .get(self.index.saturating_sub(1))
            .map_or_else(|| self.peek().span, |token| token.span)
    }
}

fn token_kind_eq(left: &TokenKind, right: &TokenKind) -> bool {
    std::mem::discriminant(left) == std::mem::discriminant(right)
}
