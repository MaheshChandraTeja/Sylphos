#![doc = "Bytecode definitions for the SylJS VM."]

use crate::{AssignOp, BinaryOp, Span, UnaryOp};

/// Constant pool entry.
#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    /// Number constant.
    Number(f64),

    /// String constant.
    String(String),

    /// Boolean constant.
    Boolean(bool),

    /// Null constant.
    Null,

    /// Undefined constant.
    Undefined,

    /// Nested function constant.
    Function(BytecodeFunction),
}

/// Compiled bytecode function.
#[derive(Debug, Clone, PartialEq)]
pub struct BytecodeFunction {
    /// Function name, if available.
    pub name: Option<String>,

    /// Function parameters.
    pub params: Vec<String>,

    /// Constant pool.
    pub constants: Vec<Constant>,

    /// Instruction stream.
    pub instructions: Vec<Instruction>,

    /// Source span.
    pub span: Span,
}

impl BytecodeFunction {
    /// Creates an empty bytecode function.
    #[must_use]
    pub fn new(name: Option<String>, params: Vec<String>, span: Span) -> Self {
        Self {
            name,
            params,
            constants: Vec::new(),
            instructions: Vec::new(),
            span,
        }
    }

    /// Adds a constant and returns its index.
    pub fn push_constant(&mut self, constant: Constant) -> u32 {
        self.constants.push(constant);
        u32::try_from(self.constants.len().saturating_sub(1)).unwrap_or(u32::MAX)
    }

    /// Adds an instruction and returns its index.
    pub fn push_instruction(&mut self, instruction: Instruction) -> usize {
        self.instructions.push(instruction);
        self.instructions.len().saturating_sub(1)
    }

    /// Patches a jump target.
    pub fn patch_jump(&mut self, instruction_index: usize, target: usize) {
        if let Some(instruction) = self.instructions.get_mut(instruction_index) {
            match instruction {
                Instruction::Jump { target: slot }
                | Instruction::JumpIfFalse { target: slot }
                | Instruction::JumpIfTruthy { target: slot } => {
                    *slot = target;
                }
                _ => {}
            }
        }
    }
}

/// Bytecode instruction.
#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    /// Push a constant by index.
    LoadConst(u32),

    /// Push undefined.
    LoadUndefined,

    /// Load a global variable/property.
    LoadGlobal(String),

    /// Store a global variable/property.
    StoreGlobal(String),

    /// Load a local/scoped binding.
    LoadName(String),

    /// Store a local/scoped binding.
    StoreName(String),

    /// Declare a scoped binding.
    DeclareName(String),

    /// Pop stack top.
    Pop,

    /// Duplicate stack top.
    Dup,

    /// Create a new object.
    NewObject,

    /// Create a new array.
    NewArray(usize),

    /// Define object property. Stack: object, value -> object.
    DefineProperty(String),

    /// Get property. Stack: object, key -> value.
    GetProperty,

    /// Set property. Stack: object, key, value -> value.
    SetProperty,

    /// Get static property. Stack: object -> value.
    GetNamedProperty(String),

    /// Set static property. Stack: object, value -> value.
    SetNamedProperty(String),

    /// Unary operation.
    Unary(UnaryOp),

    /// Binary operation.
    Binary(BinaryOp),

    /// Assignment operation helper.
    Assignment(AssignOp),

    /// Function call with argument count.
    Call(usize),

    /// Construct with argument count.
    New(usize),

    /// Unconditional jump.
    Jump {
        /// Absolute instruction target.
        target: usize,
    },

    /// Jump if falsey. Consumes test.
    JumpIfFalse {
        /// Absolute instruction target.
        target: usize,
    },

    /// Jump if truthy. Consumes test.
    JumpIfTruthy {
        /// Absolute instruction target.
        target: usize,
    },

    /// Return from current function.
    Return,

    /// Enter lexical scope.
    EnterScope,

    /// Exit lexical scope.
    ExitScope,

    /// Runtime no-op.
    Nop,
}
