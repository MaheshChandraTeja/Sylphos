#![doc = "AST visitor and statistics helpers for SylJS."]

use crate::{Expr, ExprKind, Program, Stmt, StmtKind};

/// Basic AST statistics useful for paper metrics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AstStats {
    /// Statement count.
    pub statements: usize,

    /// Expression count.
    pub expressions: usize,

    /// Function declaration/expression count.
    pub functions: usize,

    /// Call expression count.
    pub calls: usize,

    /// Member expression count.
    pub member_accesses: usize,

    /// Assignment expression count.
    pub assignments: usize,
}

/// AST visitor.
pub trait AstVisitor {
    /// Visits a program.
    fn visit_program(&mut self, program: &Program) {
        for stmt in &program.body {
            self.visit_stmt(stmt);
        }
    }

    /// Visits a statement.
    fn visit_stmt(&mut self, stmt: &Stmt) {
        walk_stmt(self, stmt);
    }

    /// Visits an expression.
    fn visit_expr(&mut self, expr: &Expr) {
        walk_expr(self, expr);
    }
}

/// Walks a statement.
pub fn walk_stmt<V: AstVisitor + ?Sized>(visitor: &mut V, stmt: &Stmt) {
    match &stmt.kind {
        StmtKind::Empty | StmtKind::Break | StmtKind::Continue => {}
        StmtKind::Block(body) => {
            for stmt in body {
                visitor.visit_stmt(stmt);
            }
        }
        StmtKind::Expr(expr) => visitor.visit_expr(expr),
        StmtKind::VarDecl(decl) => {
            for declarator in &decl.declarations {
                if let Some(init) = &declarator.init {
                    visitor.visit_expr(init);
                }
            }
        }
        StmtKind::FunctionDecl(function) => {
            for stmt in &function.body {
                visitor.visit_stmt(stmt);
            }
        }
        StmtKind::Return(expr) => {
            if let Some(expr) = expr {
                visitor.visit_expr(expr);
            }
        }
        StmtKind::If {
            test,
            consequent,
            alternate,
        } => {
            visitor.visit_expr(test);
            visitor.visit_stmt(consequent);
            if let Some(alternate) = alternate {
                visitor.visit_stmt(alternate);
            }
        }
        StmtKind::While { test, body } => {
            visitor.visit_expr(test);
            visitor.visit_stmt(body);
        }
        StmtKind::For {
            init,
            test,
            update,
            body,
        } => {
            if let Some(init) = init {
                match init {
                    crate::ast::ForInit::VarDecl(decl) => {
                        for declarator in &decl.declarations {
                            if let Some(init) = &declarator.init {
                                visitor.visit_expr(init);
                            }
                        }
                    }
                    crate::ast::ForInit::Expr(expr) => visitor.visit_expr(expr),
                }
            }
            if let Some(test) = test {
                visitor.visit_expr(test);
            }
            if let Some(update) = update {
                visitor.visit_expr(update);
            }
            visitor.visit_stmt(body);
        }
    }
}

/// Walks an expression.
pub fn walk_expr<V: AstVisitor + ?Sized>(visitor: &mut V, expr: &Expr) {
    match &expr.kind {
        ExprKind::Literal(_) | ExprKind::Identifier(_) | ExprKind::This => {}
        ExprKind::Array(items) => {
            for expr in items.iter().flatten() {
                visitor.visit_expr(expr);
            }
        }
        ExprKind::Object(properties) => {
            for property in properties {
                visitor.visit_expr(&property.value);
            }
        }
        ExprKind::Unary { argument, .. } => visitor.visit_expr(argument),
        ExprKind::Binary { left, right, .. } | ExprKind::Assign { left, right, .. } => {
            visitor.visit_expr(left);
            visitor.visit_expr(right);
        }
        ExprKind::Member { object, property } => {
            visitor.visit_expr(object);
            if let crate::ast::MemberProperty::Computed(expr) = property {
                visitor.visit_expr(expr);
            }
        }
        ExprKind::Call { callee, arguments } => {
            visitor.visit_expr(callee);
            for arg in arguments {
                visitor.visit_expr(arg);
            }
        }
        ExprKind::Function { body, .. } => {
            for stmt in body {
                visitor.visit_stmt(stmt);
            }
        }
        ExprKind::New { callee, arguments } => {
            visitor.visit_expr(callee);
            for arg in arguments {
                visitor.visit_expr(arg);
            }
        }
        ExprKind::Conditional {
            test,
            consequent,
            alternate,
        } => {
            visitor.visit_expr(test);
            visitor.visit_expr(consequent);
            visitor.visit_expr(alternate);
        }
    }
}

impl AstStats {
    /// Computes AST stats for a program.
    #[must_use]
    pub fn collect(program: &Program) -> Self {
        let mut collector = StatsCollector::default();
        collector.visit_program(program);
        collector.stats
    }
}

#[derive(Default)]
struct StatsCollector {
    stats: AstStats,
}

impl AstVisitor for StatsCollector {
    fn visit_stmt(&mut self, stmt: &Stmt) {
        self.stats.statements = self.stats.statements.saturating_add(1);

        if matches!(stmt.kind, StmtKind::FunctionDecl(_)) {
            self.stats.functions = self.stats.functions.saturating_add(1);
        }

        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &Expr) {
        self.stats.expressions = self.stats.expressions.saturating_add(1);

        match expr.kind {
            ExprKind::Call { .. } => self.stats.calls = self.stats.calls.saturating_add(1),
            ExprKind::Member { .. } => {
                self.stats.member_accesses = self.stats.member_accesses.saturating_add(1);
            }
            ExprKind::Assign { .. } => {
                self.stats.assignments = self.stats.assignments.saturating_add(1);
            }
            ExprKind::Function { .. } => {
                self.stats.functions = self.stats.functions.saturating_add(1);
            }
            _ => {}
        }

        walk_expr(self, expr);
    }
}
