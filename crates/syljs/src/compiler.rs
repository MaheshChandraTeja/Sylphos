#![doc = "AST-to-bytecode compiler for SylJS."]

use crate::{
    ast::{
        AssignOp, BindingPattern, Expr, ExprKind, ForInit, FunctionDecl, Literal, MemberProperty,
        Program, Stmt, StmtKind, VarDecl,
    },
    bytecode::{BytecodeFunction, Constant, Instruction},
    Span,
};

/// Compiler configuration.
#[derive(Debug, Clone)]
pub struct CompileOptions {
    /// Adds an implicit `return undefined` at the end of scripts/functions.
    pub implicit_return_undefined: bool,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            implicit_return_undefined: true,
        }
    }
}

/// Compiler error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    /// Human-readable message.
    pub message: String,

    /// Source span.
    pub span: Span,
}

impl CompileError {
    #[must_use]
    fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} at {}..{}",
            self.message, self.span.start, self.span.end
        )
    }
}

impl std::error::Error for CompileError {}

/// Compiles a program to a top-level bytecode function.
pub fn compile_program(
    program: &Program,
    options: CompileOptions,
) -> Result<BytecodeFunction, CompileError> {
    let mut compiler = Compiler::new(BytecodeFunction::new(
        Some("<script>".to_owned()),
        Vec::new(),
        program.span,
    ));

    for stmt in &program.body {
        compiler.compile_stmt(stmt)?;
    }

    if options.implicit_return_undefined {
        compiler
            .function
            .push_instruction(Instruction::LoadUndefined);
        compiler.function.push_instruction(Instruction::Return);
    }

    Ok(compiler.function)
}

struct Compiler {
    function: BytecodeFunction,
    loop_stack: Vec<LoopPatch>,
}

#[derive(Debug, Default)]
struct LoopPatch {
    break_jumps: Vec<usize>,
    continue_jumps: Vec<usize>,
}

impl Compiler {
    fn new(function: BytecodeFunction) -> Self {
        Self {
            function,
            loop_stack: Vec::new(),
        }
    }

    fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), CompileError> {
        match &stmt.kind {
            StmtKind::Empty => {
                self.function.push_instruction(Instruction::Nop);
            }
            StmtKind::Block(body) => {
                self.function.push_instruction(Instruction::EnterScope);
                for stmt in body {
                    self.compile_stmt(stmt)?;
                }
                self.function.push_instruction(Instruction::ExitScope);
            }
            StmtKind::Expr(expr) => {
                self.compile_expr(expr)?;
                self.function.push_instruction(Instruction::Pop);
            }
            StmtKind::VarDecl(decl) => self.compile_var_decl(decl)?,
            StmtKind::FunctionDecl(function) => {
                self.compile_function_decl(function)?;
            }
            StmtKind::Return(argument) => {
                if let Some(argument) = argument {
                    self.compile_expr(argument)?;
                } else {
                    self.function.push_instruction(Instruction::LoadUndefined);
                }
                self.function.push_instruction(Instruction::Return);
            }
            StmtKind::If {
                test,
                consequent,
                alternate,
            } => {
                self.compile_expr(test)?;
                let else_jump = self
                    .function
                    .push_instruction(Instruction::JumpIfFalse { target: usize::MAX });
                self.compile_stmt(consequent)?;
                let end_jump = self
                    .function
                    .push_instruction(Instruction::Jump { target: usize::MAX });
                let else_target = self.function.instructions.len();
                self.function.patch_jump(else_jump, else_target);
                if let Some(alternate) = alternate {
                    self.compile_stmt(alternate)?;
                }
                let end_target = self.function.instructions.len();
                self.function.patch_jump(end_jump, end_target);
            }
            StmtKind::While { test, body } => {
                let loop_start = self.function.instructions.len();
                self.compile_expr(test)?;
                let exit_jump = self
                    .function
                    .push_instruction(Instruction::JumpIfFalse { target: usize::MAX });

                self.loop_stack.push(LoopPatch::default());
                self.compile_stmt(body)?;
                let patch = self.loop_stack.pop().unwrap_or_default();

                self.function
                    .push_instruction(Instruction::Jump { target: loop_start });
                let loop_end = self.function.instructions.len();
                self.function.patch_jump(exit_jump, loop_end);

                for jump in patch.break_jumps {
                    self.function.patch_jump(jump, loop_end);
                }
                for jump in patch.continue_jumps {
                    self.function.patch_jump(jump, loop_start);
                }
            }
            StmtKind::For {
                init,
                test,
                update,
                body,
            } => {
                self.function.push_instruction(Instruction::EnterScope);

                if let Some(init) = init {
                    match init {
                        ForInit::VarDecl(decl) => self.compile_var_decl(decl)?,
                        ForInit::Expr(expr) => {
                            self.compile_expr(expr)?;
                            self.function.push_instruction(Instruction::Pop);
                        }
                    }
                }

                let loop_start = self.function.instructions.len();

                let exit_jump = if let Some(test) = test {
                    self.compile_expr(test)?;
                    Some(
                        self.function
                            .push_instruction(Instruction::JumpIfFalse { target: usize::MAX }),
                    )
                } else {
                    None
                };

                self.loop_stack.push(LoopPatch::default());
                self.compile_stmt(body)?;
                let continue_target = self.function.instructions.len();

                if let Some(update) = update {
                    self.compile_expr(update)?;
                    self.function.push_instruction(Instruction::Pop);
                }

                self.function
                    .push_instruction(Instruction::Jump { target: loop_start });
                let loop_end = self.function.instructions.len();

                if let Some(exit_jump) = exit_jump {
                    self.function.patch_jump(exit_jump, loop_end);
                }

                let patch = self.loop_stack.pop().unwrap_or_default();
                for jump in patch.break_jumps {
                    self.function.patch_jump(jump, loop_end);
                }
                for jump in patch.continue_jumps {
                    self.function.patch_jump(jump, continue_target);
                }

                self.function.push_instruction(Instruction::ExitScope);
            }
            StmtKind::Break => {
                let Some(loop_patch) = self.loop_stack.last_mut() else {
                    return Err(CompileError::new("`break` outside loop", stmt.span));
                };
                let jump = self
                    .function
                    .push_instruction(Instruction::Jump { target: usize::MAX });
                loop_patch.break_jumps.push(jump);
            }
            StmtKind::Continue => {
                let Some(loop_patch) = self.loop_stack.last_mut() else {
                    return Err(CompileError::new("`continue` outside loop", stmt.span));
                };
                let jump = self
                    .function
                    .push_instruction(Instruction::Jump { target: usize::MAX });
                loop_patch.continue_jumps.push(jump);
            }
        }

        Ok(())
    }

    fn compile_var_decl(&mut self, decl: &VarDecl) -> Result<(), CompileError> {
        let _kind = decl.kind;

        for declarator in &decl.declarations {
            let BindingPattern::Identifier(name) = &declarator.id;

            self.function
                .push_instruction(Instruction::DeclareName(name.clone()));

            if let Some(init) = &declarator.init {
                self.compile_expr(init)?;
            } else {
                self.function.push_instruction(Instruction::LoadUndefined);
            }

            self.function
                .push_instruction(Instruction::StoreName(name.clone()));

            // StoreName leaves the value on the stack for assignment expression parity.
            self.function.push_instruction(Instruction::Pop);
        }

        Ok(())
    }

    fn compile_function_decl(&mut self, function: &FunctionDecl) -> Result<(), CompileError> {
        let compiled = compile_function(function)?;
        let constant = self.function.push_constant(Constant::Function(compiled));
        self.function
            .push_instruction(Instruction::LoadConst(constant));
        self.function
            .push_instruction(Instruction::StoreName(function.name.clone()));
        self.function.push_instruction(Instruction::Pop);
        Ok(())
    }

    fn compile_expr(&mut self, expr: &Expr) -> Result<(), CompileError> {
        match &expr.kind {
            ExprKind::Literal(literal) => {
                let constant = match literal {
                    Literal::Number(value) => Constant::Number(*value),
                    Literal::String(value) => Constant::String(value.clone()),
                    Literal::Boolean(value) => Constant::Boolean(*value),
                    Literal::Null => Constant::Null,
                    Literal::Undefined => Constant::Undefined,
                };
                let index = self.function.push_constant(constant);
                self.function
                    .push_instruction(Instruction::LoadConst(index));
            }
            ExprKind::Identifier(name) => {
                self.function
                    .push_instruction(Instruction::LoadName(name.clone()));
            }
            ExprKind::This => {
                self.function
                    .push_instruction(Instruction::LoadName("this".to_owned()));
            }
            ExprKind::Array(items) => {
                for item in items {
                    if let Some(item) = item {
                        self.compile_expr(item)?;
                    } else {
                        self.function.push_instruction(Instruction::LoadUndefined);
                    }
                }
                self.function
                    .push_instruction(Instruction::NewArray(items.len()));
            }
            ExprKind::Object(properties) => {
                self.function.push_instruction(Instruction::NewObject);
                for property in properties {
                    self.compile_expr(&property.value)?;
                    self.function
                        .push_instruction(Instruction::DefineProperty(property.key.clone()));
                }
            }
            ExprKind::Unary { op, argument } => {
                self.compile_expr(argument)?;
                self.function.push_instruction(Instruction::Unary(*op));
            }
            ExprKind::Binary { op, left, right } => {
                self.compile_expr(left)?;
                self.compile_expr(right)?;
                self.function.push_instruction(Instruction::Binary(*op));
            }
            ExprKind::Assign { op, left, right } => {
                self.compile_assignment(*op, left, right)?;
            }
            ExprKind::Member { object, property } => {
                self.compile_expr(object)?;
                match property {
                    MemberProperty::Ident(name) => {
                        self.function
                            .push_instruction(Instruction::GetNamedProperty(name.clone()));
                    }
                    MemberProperty::Computed(expr) => {
                        self.compile_expr(expr)?;
                        self.function.push_instruction(Instruction::GetProperty);
                    }
                }
            }
            ExprKind::Call { callee, arguments } => {
                self.compile_expr(callee)?;
                for arg in arguments {
                    self.compile_expr(arg)?;
                }
                self.function
                    .push_instruction(Instruction::Call(arguments.len()));
            }
            ExprKind::Function { name, params, body } => {
                let nested = compile_function_parts(
                    name.clone(),
                    params.iter().map(|param| param.name.clone()).collect(),
                    body,
                    expr.span,
                )?;
                let constant = self.function.push_constant(Constant::Function(nested));
                self.function
                    .push_instruction(Instruction::LoadConst(constant));
            }
            ExprKind::New { callee, arguments } => {
                self.compile_expr(callee)?;
                for arg in arguments {
                    self.compile_expr(arg)?;
                }
                self.function
                    .push_instruction(Instruction::New(arguments.len()));
            }
            ExprKind::Conditional {
                test,
                consequent,
                alternate,
            } => {
                self.compile_expr(test)?;
                let else_jump = self
                    .function
                    .push_instruction(Instruction::JumpIfFalse { target: usize::MAX });
                self.compile_expr(consequent)?;
                let end_jump = self
                    .function
                    .push_instruction(Instruction::Jump { target: usize::MAX });
                let else_target = self.function.instructions.len();
                self.function.patch_jump(else_jump, else_target);
                self.compile_expr(alternate)?;
                let end_target = self.function.instructions.len();
                self.function.patch_jump(end_jump, end_target);
            }
        }

        Ok(())
    }

    fn compile_assignment(
        &mut self,
        op: AssignOp,
        left: &Expr,
        right: &Expr,
    ) -> Result<(), CompileError> {
        match &left.kind {
            ExprKind::Identifier(name) => {
                if op == AssignOp::Assign {
                    self.compile_expr(right)?;
                } else {
                    self.function
                        .push_instruction(Instruction::LoadName(name.clone()));
                    self.compile_expr(right)?;
                    self.function.push_instruction(Instruction::Assignment(op));
                }
                self.function
                    .push_instruction(Instruction::StoreName(name.clone()));
            }
            ExprKind::Member { object, property } => {
                self.compile_expr(object)?;
                match property {
                    MemberProperty::Ident(name) => {
                        if op == AssignOp::Assign {
                            self.compile_expr(right)?;
                        } else {
                            self.function.push_instruction(Instruction::Dup);
                            self.function
                                .push_instruction(Instruction::GetNamedProperty(name.clone()));
                            self.compile_expr(right)?;
                            self.function.push_instruction(Instruction::Assignment(op));
                        }
                        self.function
                            .push_instruction(Instruction::SetNamedProperty(name.clone()));
                    }
                    MemberProperty::Computed(property_expr) => {
                        self.compile_expr(property_expr)?;
                        if op == AssignOp::Assign {
                            self.compile_expr(right)?;
                        } else {
                            return Err(CompileError::new(
                                "compound assignment to computed properties is not supported in Module 29",
                                left.span,
                            ));
                        }
                        self.function.push_instruction(Instruction::SetProperty);
                    }
                }
            }
            _ => {
                return Err(CompileError::new("invalid assignment target", left.span));
            }
        }

        Ok(())
    }
}

fn compile_function(function: &FunctionDecl) -> Result<BytecodeFunction, CompileError> {
    compile_function_parts(
        Some(function.name.clone()),
        function
            .params
            .iter()
            .map(|param| param.name.clone())
            .collect(),
        &function.body,
        function.span,
    )
}

fn compile_function_parts(
    name: Option<String>,
    params: Vec<String>,
    body: &[Stmt],
    span: Span,
) -> Result<BytecodeFunction, CompileError> {
    let mut compiler = Compiler::new(BytecodeFunction::new(name, params, span));

    for stmt in body {
        compiler.compile_stmt(stmt)?;
    }

    compiler
        .function
        .push_instruction(Instruction::LoadUndefined);
    compiler.function.push_instruction(Instruction::Return);

    Ok(compiler.function)
}
