#![doc = "Stack-based bytecode VM for SylJS."]

use crate::{
    bytecode::{BytecodeFunction, Constant, Instruction},
    value::{JsFunction, JsNativeFunction, JsObject, JsObjectKind, JsRuntimeError, JsValue},
    AssignOp, BinaryOp, UnaryOp,
};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, time::Instant};

/// VM runtime configuration.
#[derive(Debug, Clone)]
pub struct VmConfig {
    /// Maximum instructions per execution.
    pub instruction_budget: u64,

    /// Maximum call depth.
    pub max_call_depth: usize,
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            instruction_budget: 250_000,
            max_call_depth: 256,
        }
    }
}

/// Execution metrics for research instrumentation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VmMetrics {
    /// Instructions executed.
    pub instructions_executed: u64,

    /// Function calls.
    pub calls: u64,

    /// Native calls.
    pub native_calls: u64,

    /// Bytecode calls.
    pub bytecode_calls: u64,

    /// Property reads.
    pub property_reads: u64,

    /// Property writes.
    pub property_writes: u64,

    /// Execution time in microseconds.
    pub elapsed_us: u128,
}

/// Execution result.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionOutcome {
    /// Returned value.
    pub value: JsValue,

    /// Metrics.
    pub metrics: VmMetrics,

    /// Captured console lines.
    pub console: Vec<String>,
}

/// SylJS VM.
pub struct Vm {
    /// Runtime configuration.
    pub config: VmConfig,

    /// Global object.
    pub global: JsValue,

    scopes: Vec<BTreeMap<String, JsValue>>,
    stack: Vec<JsValue>,
    call_depth: usize,
    metrics: VmMetrics,
    console: Vec<String>,
}

impl Default for Vm {
    fn default() -> Self {
        let mut vm = Self {
            config: VmConfig::default(),
            global: JsValue::object(),
            scopes: vec![BTreeMap::new()],
            stack: Vec::new(),
            call_depth: 0,
            metrics: VmMetrics::default(),
            console: Vec::new(),
        };
        vm.install_standard_globals();
        vm
    }
}

impl Vm {
    /// Creates a VM with config.
    #[must_use]
    pub fn with_config(config: VmConfig) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }

    /// Executes a bytecode function as a top-level script.
    pub fn execute(
        &mut self,
        function: &BytecodeFunction,
    ) -> Result<ExecutionOutcome, JsRuntimeError> {
        let started = Instant::now();
        self.metrics = VmMetrics::default();
        self.console.clear();
        self.stack.clear();

        let value = self.execute_function(function, JsValue::Undefined, Vec::new())?;
        self.metrics.elapsed_us = started.elapsed().as_micros();

        Ok(ExecutionOutcome {
            value,
            metrics: self.metrics.clone(),
            console: self.console.clone(),
        })
    }

    /// Calls a JavaScript/native function value.
    ///
    /// Event-loop modules use this to execute queued timer, microtask, Promise,
    /// and animation-frame callbacks.
    pub fn call_function(
        &mut self,
        function: JsValue,
        this_value: JsValue,
        args: Vec<JsValue>,
    ) -> Result<JsValue, JsRuntimeError> {
        self.call_value(function, this_value, args)
    }

    /// Returns metrics accumulated since the last top-level execution reset.
    #[must_use]
    pub fn metrics(&self) -> VmMetrics {
        self.metrics.clone()
    }

    /// Returns captured console lines.
    #[must_use]
    pub fn console(&self) -> &[String] {
        &self.console
    }

    /// Drains captured console lines.
    pub fn drain_console(&mut self) -> Vec<String> {
        std::mem::take(&mut self.console)
    }

    /// Defines a global value.
    pub fn define_global(&mut self, name: impl Into<String>, value: JsValue) {
        let name = name.into();
        if let JsValue::Object(object) = &self.global {
            object.borrow_mut().set(name.clone(), value.clone());
        }
        if let Some(scope) = self.scopes.first_mut() {
            scope.insert(name, value);
        }
    }

    /// Defines a native global function.
    pub fn define_native_function(&mut self, name: impl Into<String>, function: JsNativeFunction) {
        let name = name.into();
        self.define_global(
            name.clone(),
            JsValue::function(JsFunction::Native { name, function }),
        );
    }

    /// Appends a console line.
    pub fn push_console_line(&mut self, line: impl Into<String>) {
        self.console.push(line.into());
    }

    /// Reads a global/scoped value.
    #[must_use]
    pub fn get_name(&self, name: &str) -> JsValue {
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return value.clone();
            }
        }
        self.global.get_property(name)
    }

    fn execute_function(
        &mut self,
        function: &BytecodeFunction,
        this_value: JsValue,
        args: Vec<JsValue>,
    ) -> Result<JsValue, JsRuntimeError> {
        if self.call_depth >= self.config.max_call_depth {
            return Err(JsRuntimeError::new("maximum SylJS call depth exceeded"));
        }

        self.call_depth = self.call_depth.saturating_add(1);
        self.scopes.push(BTreeMap::new());

        self.set_name("this", this_value);

        for (index, param) in function.params.iter().enumerate() {
            let value = args.get(index).cloned().unwrap_or(JsValue::Undefined);
            self.set_name(param, value);
        }

        let mut ip = 0usize;
        let mut returned = JsValue::Undefined;

        while ip < function.instructions.len() {
            self.metrics.instructions_executed =
                self.metrics.instructions_executed.saturating_add(1);

            if self.metrics.instructions_executed > self.config.instruction_budget {
                self.call_depth = self.call_depth.saturating_sub(1);
                let _ = self.scopes.pop();
                return Err(JsRuntimeError::new("SylJS instruction budget exceeded"));
            }

            match &function.instructions[ip] {
                Instruction::LoadConst(index) => {
                    let value = self.constant_to_value(function, *index)?;
                    self.stack.push(value);
                }
                Instruction::LoadUndefined => self.stack.push(JsValue::Undefined),
                Instruction::LoadGlobal(name) | Instruction::LoadName(name) => {
                    self.stack.push(self.get_name(name));
                }
                Instruction::StoreGlobal(name) | Instruction::StoreName(name) => {
                    let value = self.peek_stack()?.clone();
                    self.set_name(name, value);
                }
                Instruction::DeclareName(name) => {
                    if let Some(scope) = self.scopes.last_mut() {
                        scope.entry(name.clone()).or_insert(JsValue::Undefined);
                    }
                }
                Instruction::Pop => {
                    let _ = self.stack.pop();
                }
                Instruction::Dup => {
                    let value = self.peek_stack()?.clone();
                    self.stack.push(value);
                }
                Instruction::NewObject => {
                    self.stack.push(JsValue::object());
                }
                Instruction::NewArray(count) => {
                    let mut items = Vec::with_capacity(*count);
                    for _ in 0..*count {
                        items.push(self.pop_stack()?);
                    }
                    items.reverse();
                    self.stack.push(JsValue::array(items));
                }
                Instruction::DefineProperty(key) => {
                    let value = self.pop_stack()?;
                    let object = self.peek_stack()?.clone();
                    object.set_property(key.clone(), value);
                    self.metrics.property_writes = self.metrics.property_writes.saturating_add(1);
                }
                Instruction::GetProperty => {
                    let key = self.pop_stack()?.to_js_string();
                    let object = self.pop_stack()?;
                    self.metrics.property_reads = self.metrics.property_reads.saturating_add(1);
                    self.stack.push(object.get_property(&key));
                }
                Instruction::SetProperty => {
                    let value = self.pop_stack()?;
                    let key = self.pop_stack()?.to_js_string();
                    let object = self.pop_stack()?;
                    object.set_property(key, value.clone());
                    self.metrics.property_writes = self.metrics.property_writes.saturating_add(1);
                    self.stack.push(value);
                }
                Instruction::GetNamedProperty(key) => {
                    let object = self.pop_stack()?;
                    self.metrics.property_reads = self.metrics.property_reads.saturating_add(1);
                    self.stack.push(object.get_property(key));
                }
                Instruction::SetNamedProperty(key) => {
                    let value = self.pop_stack()?;
                    let object = self.pop_stack()?;
                    object.set_property(key.clone(), value.clone());
                    self.metrics.property_writes = self.metrics.property_writes.saturating_add(1);
                    self.stack.push(value);
                }
                Instruction::Unary(op) => {
                    let value = self.pop_stack()?;
                    self.stack.push(apply_unary(*op, value));
                }
                Instruction::Binary(op) => {
                    let right = self.pop_stack()?;
                    let left = self.pop_stack()?;
                    self.stack.push(apply_binary(*op, left, right));
                }
                Instruction::Assignment(op) => {
                    let right = self.pop_stack()?;
                    let left = self.pop_stack()?;
                    self.stack.push(apply_assignment(*op, left, right));
                }
                Instruction::Call(count) => {
                    let args = self.pop_args(*count)?;
                    let callee = self.pop_stack()?;
                    let value = self.call_value(callee, JsValue::Undefined, args)?;
                    self.stack.push(value);
                }
                Instruction::New(count) => {
                    let args = self.pop_args(*count)?;
                    let callee = self.pop_stack()?;
                    let value = self.call_value(callee, JsValue::object(), args)?;
                    self.stack.push(value);
                }
                Instruction::Jump { target } => {
                    ip = *target;
                    continue;
                }
                Instruction::JumpIfFalse { target } => {
                    let test = self.pop_stack()?;
                    if !test.is_truthy() {
                        ip = *target;
                        continue;
                    }
                }
                Instruction::JumpIfTruthy { target } => {
                    let test = self.pop_stack()?;
                    if test.is_truthy() {
                        ip = *target;
                        continue;
                    }
                }
                Instruction::Return => {
                    returned = self.stack.pop().unwrap_or(JsValue::Undefined);
                    break;
                }
                Instruction::EnterScope => self.scopes.push(BTreeMap::new()),
                Instruction::ExitScope => {
                    if self.scopes.len() > 1 {
                        let _ = self.scopes.pop();
                    }
                }
                Instruction::Nop => {}
            }

            ip = ip.saturating_add(1);
        }

        if self.scopes.len() > 1 {
            let _ = self.scopes.pop();
        }
        self.call_depth = self.call_depth.saturating_sub(1);

        Ok(returned)
    }

    fn call_value(
        &mut self,
        callee: JsValue,
        this_value: JsValue,
        args: Vec<JsValue>,
    ) -> Result<JsValue, JsRuntimeError> {
        self.metrics.calls = self.metrics.calls.saturating_add(1);

        let Some(function) = callee.as_function() else {
            return Err(JsRuntimeError::new(format!(
                "`{}` is not callable",
                callee.to_js_string()
            )));
        };

        match function {
            JsFunction::Bytecode(function) => {
                self.metrics.bytecode_calls = self.metrics.bytecode_calls.saturating_add(1);
                self.execute_function(&function, this_value, args)
            }
            JsFunction::Native { function, .. } => {
                self.metrics.native_calls = self.metrics.native_calls.saturating_add(1);
                function(self, this_value, args)
            }
        }
    }

    fn constant_to_value(
        &self,
        function: &BytecodeFunction,
        index: u32,
    ) -> Result<JsValue, JsRuntimeError> {
        let constant = function
            .constants
            .get(index as usize)
            .ok_or_else(|| JsRuntimeError::new("constant index out of range"))?;

        Ok(match constant {
            Constant::Number(value) => JsValue::Number(*value),
            Constant::String(value) => JsValue::String(value.clone()),
            Constant::Boolean(value) => JsValue::Boolean(*value),
            Constant::Null => JsValue::Null,
            Constant::Undefined => JsValue::Undefined,
            Constant::Function(function) => {
                JsValue::function(JsFunction::Bytecode(Rc::new(function.clone())))
            }
        })
    }

    fn set_name(&mut self, name: &str, value: JsValue) {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_owned(), value.clone());
                self.global.set_property(name.to_owned(), value);
                return;
            }
        }

        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_owned(), value.clone());
        }
        self.global.set_property(name.to_owned(), value);
    }

    fn pop_args(&mut self, count: usize) -> Result<Vec<JsValue>, JsRuntimeError> {
        let mut args = Vec::with_capacity(count);

        for _ in 0..count {
            args.push(self.pop_stack()?);
        }

        args.reverse();
        Ok(args)
    }

    fn pop_stack(&mut self) -> Result<JsValue, JsRuntimeError> {
        self.stack
            .pop()
            .ok_or_else(|| JsRuntimeError::new("SylJS stack underflow"))
    }

    fn peek_stack(&self) -> Result<&JsValue, JsRuntimeError> {
        self.stack
            .last()
            .ok_or_else(|| JsRuntimeError::new("SylJS stack underflow"))
    }

    fn install_standard_globals(&mut self) {
        let console = JsValue::Object(Rc::new(RefCell::new(JsObject::new(JsObjectKind::Host))));

        let log_fn: JsNativeFunction = Rc::new(|vm, _this, args| {
            let line = args
                .iter()
                .map(JsValue::to_js_string)
                .collect::<Vec<_>>()
                .join(" ");
            vm.push_console_line(line);
            Ok(JsValue::Undefined)
        });

        console.set_property(
            "log",
            JsValue::function(JsFunction::Native {
                name: "console.log".to_owned(),
                function: log_fn.clone(),
            }),
        );
        console.set_property(
            "info",
            JsValue::function(JsFunction::Native {
                name: "console.info".to_owned(),
                function: log_fn.clone(),
            }),
        );
        console.set_property(
            "warn",
            JsValue::function(JsFunction::Native {
                name: "console.warn".to_owned(),
                function: log_fn.clone(),
            }),
        );
        console.set_property(
            "error",
            JsValue::function(JsFunction::Native {
                name: "console.error".to_owned(),
                function: log_fn,
            }),
        );

        self.define_global("console", console);

        let parse_float: JsNativeFunction = Rc::new(|_vm, _this, args| {
            let value = args.first().map_or(f64::NAN, |value| {
                value.to_js_string().parse::<f64>().unwrap_or(f64::NAN)
            });
            Ok(JsValue::Number(value))
        });
        self.define_native_function("parseFloat", parse_float);
    }
}

fn apply_unary(op: UnaryOp, value: JsValue) -> JsValue {
    match op {
        UnaryOp::Not => JsValue::Boolean(!value.is_truthy()),
        UnaryOp::Neg => JsValue::Number(-value.to_number()),
        UnaryOp::Pos => JsValue::Number(value.to_number()),
        UnaryOp::Typeof => JsValue::String(
            match value {
                JsValue::Undefined => "undefined",
                JsValue::Null => "object",
                JsValue::Boolean(_) => "boolean",
                JsValue::Number(_) => "number",
                JsValue::String(_) => "string",
                JsValue::Object(_) => "object",
            }
            .to_owned(),
        ),
        UnaryOp::Void => JsValue::Undefined,
        UnaryOp::Delete => JsValue::Boolean(true),
    }
}

fn apply_binary(op: BinaryOp, left: JsValue, right: JsValue) -> JsValue {
    match op {
        BinaryOp::Add => {
            if matches!(left, JsValue::String(_)) || matches!(right, JsValue::String(_)) {
                JsValue::String(format!("{}{}", left.to_js_string(), right.to_js_string()))
            } else {
                JsValue::Number(left.to_number() + right.to_number())
            }
        }
        BinaryOp::Sub => JsValue::Number(left.to_number() - right.to_number()),
        BinaryOp::Mul => JsValue::Number(left.to_number() * right.to_number()),
        BinaryOp::Div => JsValue::Number(left.to_number() / right.to_number()),
        BinaryOp::Mod => JsValue::Number(left.to_number() % right.to_number()),
        BinaryOp::Eq | BinaryOp::StrictEq => JsValue::Boolean(left == right),
        BinaryOp::NotEq | BinaryOp::StrictNotEq => JsValue::Boolean(left != right),
        BinaryOp::Lt => JsValue::Boolean(left.to_number() < right.to_number()),
        BinaryOp::Lte => JsValue::Boolean(left.to_number() <= right.to_number()),
        BinaryOp::Gt => JsValue::Boolean(left.to_number() > right.to_number()),
        BinaryOp::Gte => JsValue::Boolean(left.to_number() >= right.to_number()),
        BinaryOp::LogicalAnd => {
            if left.is_truthy() {
                right
            } else {
                left
            }
        }
        BinaryOp::LogicalOr => {
            if left.is_truthy() {
                left
            } else {
                right
            }
        }
    }
}

fn apply_assignment(op: AssignOp, left: JsValue, right: JsValue) -> JsValue {
    match op {
        AssignOp::Assign => right,
        AssignOp::AddAssign => apply_binary(BinaryOp::Add, left, right),
        AssignOp::SubAssign => apply_binary(BinaryOp::Sub, left, right),
        AssignOp::MulAssign => apply_binary(BinaryOp::Mul, left, right),
        AssignOp::DivAssign => apply_binary(BinaryOp::Div, left, right),
        AssignOp::ModAssign => apply_binary(BinaryOp::Mod, left, right),
    }
}
