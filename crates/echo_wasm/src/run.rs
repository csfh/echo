//! Playground host: lower checked virtual source to MIR and execute it.
//!
//! This is a browser/demo host, not `xo run`. Unsupported native services
//! (`fs`, net, process, tasks) fail with a playground-host error.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use echo_ast::{BinaryOp, UnaryOp};
use echo_hir::lower_file;
use echo_mir::{
    CallTarget, LoweredProgram, MirCfg, MirExpr, MirFn, MirOp, MirPrim, MirProgram, MirRetShape,
    ModuleLowerInput, StrPart, TAG_ERR, TAG_NONE, Terminator, lower_program,
};
use echo_resolver::{ProjectChecked, VirtualSources, check_entry_virtual, module_bind_name};
use echo_runtime::{
    bytes_handle_from_slice, echo_runtime_bytes_cat, echo_runtime_bytes_from_value,
    echo_runtime_eprint, echo_runtime_float_from_f64, echo_runtime_list_new,
    echo_runtime_locator_from_string, echo_runtime_print_i64, echo_runtime_range_new,
    echo_runtime_str_from_debug, echo_runtime_str_from_int, echo_runtime_string_builder_finish,
    echo_runtime_string_builder_new, echo_runtime_string_builder_push_value, list_get_value,
    list_len_value, list_push_value, list_set_value, string_builder_push_utf8,
    string_handle_from_utf8, struct_get_value, struct_new_value, struct_set_value,
    struct_type_is_value, with_print_capture,
};
use echo_semantics::SemanticModel;
use echo_source::{BytePos, SourceId, Span};
use echo_std::is_runtime_module_path;
use serde::Serialize;

use crate::{CheckDiagnostic, PLAYGROUND_PATH, check_source, playground_workspace};

const MAX_STEPS: u32 = 2_000_000;
const MAX_CALL_DEPTH: u32 = 256;

/// `/try` Sum sample (must stay in lockstep with `www/src/docs/site.ts` `homePage.sample`).
pub const SAMPLE_SUM: &str = r#"/ std/io

$ xs = [1, 2, 3]
~ sum = 0
* x : xs {
    ~ sum = sum + x
}
io.print("sum={sum}")
"#;

/// Homepage `sum.echo` figure: same program as [`SAMPLE_SUM`], no trailing newline.
pub const HOMEPAGE_SUM: &str = r#"/ std/io

$ xs = [1, 2, 3]
~ sum = 0
* x : xs {
    ~ sum = sum + x
}
io.print("sum={sum}")"#;

/// `/try` Result sample.
pub const SAMPLE_RESULT: &str = r#"/ std/io
/ std/str

$ checked = (x) {
    ? x < 0 {
        ! 99
    }
    ^ x
}

| checked(7) {
    $ v {
        io.print(str.from_int(v))
    }
    ! e {
        io.print(str.from_int(e))
    }
}
"#;

/// `/try` Struct sample.
pub const SAMPLE_STRUCT: &str = r#"/ std/io
/ std/str

% point {
    ~ x
    ~ y
}

$ p = point { x: 3, y: 4 }
io.print(str.from_int(p.x))
~ p.x = p.x + 10
io.print(str.from_int(p.x))
"#;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RunResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub printed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_error: Option<String>,
    pub diagnostics: Vec<CheckDiagnostic>,
}

/// Check, then execute MIR with `io.print` capture. Check failures do not run.
#[must_use]
pub fn run_source(source: &str) -> RunResult {
    let checked = check_source(source);
    if !checked.ok {
        return RunResult {
            ok: false,
            printed: None,
            host_error: None,
            diagnostics: checked.diagnostics,
        };
    }

    let (sources, search) = playground_workspace(source);
    let project = check_entry_virtual(Path::new(PLAYGROUND_PATH), &search, &sources);
    match execute_checked(&project, &sources) {
        Ok(printed) => RunResult {
            ok: true,
            printed: Some(printed),
            host_error: None,
            diagnostics: Vec::new(),
        },
        Err(err) => RunResult {
            ok: false,
            printed: None,
            host_error: Some(err),
            diagnostics: Vec::new(),
        },
    }
}

#[must_use]
pub fn run_json(source: &str) -> String {
    serde_json::to_string(&run_source(source)).expect("run json")
}

fn execute_checked(project: &ProjectChecked, _sources: &VirtualSources) -> Result<String, String> {
    if project.diagnostics.error_count() > 0 {
        return Err("playground-host: program did not check".into());
    }
    let lowered = lower_checked(project)?;
    let (outcome, printed) = with_print_capture(|| interpret_program(&lowered.program));
    outcome?;
    Ok(printed)
}

fn lower_checked(project: &ProjectChecked) -> Result<LoweredProgram, String> {
    let inputs = package_modules(project);
    let lowered = lower_program(project.graph.entry.clone(), &inputs);
    if lowered.diagnostics.error_count() > 0 {
        return Err(format!(
            "playground-host: cannot lower ({})",
            lowered
                .diagnostics
                .items()
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    Ok(lowered)
}

pub(crate) fn package_modules(checked: &ProjectChecked) -> Vec<ModuleLowerInput> {
    let mut out = Vec::new();
    for unit in &checked.graph.modules {
        if is_runtime_module_path(&unit.path) {
            continue;
        }
        let mut imports = HashMap::new();
        let mut import_names = std::collections::HashSet::new();
        for (imp, target) in &unit.import_targets {
            if let Some(name) = module_bind_name(&imp.segments) {
                import_names.insert(name.clone());
                imports.insert(name, target.clone());
            }
        }
        let exports: Vec<String> = unit.facts.exports.iter().map(|e| e.name.clone()).collect();
        let hir = match unit.parsed.file.as_ref() {
            Some(f) => lower_file(f, &import_names),
            None => echo_hir::HirModule {
                import_modules: import_names.clone(),
                ..echo_hir::HirModule::default()
            },
        };
        let semantic = slim_semantic(&import_names);
        out.push(ModuleLowerInput {
            path: unit.path.clone(),
            hir,
            imports,
            exports,
            semantic,
        });
    }
    out
}

fn slim_semantic(import_names: &std::collections::HashSet<String>) -> SemanticModel {
    let mut model = SemanticModel::new();
    let dummy = Span::new(SourceId::from_u32(0), BytePos(0), BytePos(0));
    for name in import_names {
        model.introduce_in_scope(
            name.clone(),
            echo_semantics::BindingKind::Module,
            echo_semantics::ValueKind::Module,
            dummy,
            0,
        );
    }
    model
}

fn runtime_export_allowed(export: &str) -> bool {
    matches!(
        export,
        "print"
            | "eprint"
            | "str_from_int"
            | "str_from_float"
            | "str_from_bytes"
            | "str_from_duration"
            | "str_from_locator"
            | "str_from_debug"
            | "str_len"
            | "str_cat"
            | "str_trim"
            | "str_to_lower"
            | "str_to_upper"
            | "str_contains"
            | "str_starts_with"
            | "str_ends_with"
            | "str_slice"
            | "str_get"
            | "str_repeat"
            | "str_split"
            | "str_replace"
            | "list_len"
            | "list_get"
            | "list_new"
            | "list_push"
            | "list_reserve"
    )
}

#[derive(Clone)]
enum Slot {
    I64(i64),
    Tagged {
        tag: i64,
        payload: i64,
    },
    Fn {
        module_path: PathBuf,
        symbol: String,
    },
}

impl Slot {
    fn i64(&self) -> Result<i64, String> {
        match self {
            Self::I64(v) | Self::Tagged { payload: v, .. } => Ok(*v),
            Self::Fn { symbol, .. } => Err(format!("playground-host: {symbol} is a function")),
        }
    }
}

fn eval_bytes_interp(parts: &[StrPart], env: &HashMap<String, Slot>) -> Result<i64, String> {
    let mut acc: Option<i64> = None;
    for part in parts {
        let chunk = match part {
            StrPart::Lit(bytes) => bytes_handle_from_slice(bytes),
            StrPart::Name(n) => {
                let v = env
                    .get(n)
                    .cloned()
                    .ok_or_else(|| format!("playground-host: unbound {n}"))?
                    .i64()?;
                echo_runtime_bytes_from_value(v)
            }
        };
        acc = Some(match acc {
            None => chunk,
            Some(prev) => echo_runtime_bytes_cat(prev, chunk),
        });
    }
    Ok(acc.unwrap_or_else(|| bytes_handle_from_slice(&[])))
}

fn eval_string_interp(parts: &[StrPart], env: &HashMap<String, Slot>) -> Result<i64, String> {
    let b = echo_runtime_string_builder_new();
    for part in parts {
        match part {
            StrPart::Lit(bytes) => {
                let s = String::from_utf8_lossy(bytes);
                string_builder_push_utf8(b, &s);
            }
            StrPart::Name(n) => {
                let v = env
                    .get(n)
                    .cloned()
                    .ok_or_else(|| format!("playground-host: unbound {n}"))?
                    .i64()?;
                echo_runtime_string_builder_push_value(b, v);
            }
        }
    }
    Ok(echo_runtime_string_builder_finish(b))
}

struct Machine<'a> {
    program: &'a MirProgram,
    steps: u32,
}

fn interpret_program(program: &MirProgram) -> Result<(), String> {
    let mut m = Machine { program, steps: 0 };
    let entry = program
        .functions
        .iter()
        .find(|f| f.name == "__toplevel" && f.module_path == program.entry_path)
        .or_else(|| program.functions.iter().find(|f| f.name == "__toplevel"))
        .ok_or_else(|| "playground-host: no entry body".to_string())?;
    m.call_fn(entry, &[])?;
    Ok(())
}

impl<'a> Machine<'a> {
    fn call_fn(&mut self, func: &MirFn, args: &[i64]) -> Result<Slot, String> {
        if args.len() != func.params.len() {
            return Err(format!(
                "playground-host: arity mismatch calling {}",
                func.name
            ));
        }
        let mut env: HashMap<String, Slot> = HashMap::new();
        for (p, a) in func.params.iter().zip(args.iter()) {
            let slot = Slot::I64(*a);
            env.insert(p.clone(), slot.clone());
            // SSA rename of params starts at `@0`.
            env.insert(format!("{p}@0"), slot);
        }
        self.exec_cfg(&func.cfg, func.ret, &mut env, 0)
    }

    fn exec_cfg(
        &mut self,
        cfg: &MirCfg,
        ret: MirRetShape,
        env: &mut HashMap<String, Slot>,
        depth: u32,
    ) -> Result<Slot, String> {
        if depth > MAX_CALL_DEPTH {
            return Err("playground-host: call stack limit".into());
        }
        let mut bb = cfg.entry;
        let mut pred: Option<echo_mir::BlockId> = None;
        loop {
            self.steps += 1;
            if self.steps > MAX_STEPS {
                return Err("playground-host: step limit".into());
            }
            let block = cfg.block(bb);
            for op in &block.ops {
                if let MirOp::Phi { name, incomings } = op {
                    if let Some(p) = pred {
                        if let Some((_, src)) = incomings.iter().find(|(b, _)| *b == p) {
                            if let Some(v) = env.get(src).cloned() {
                                env.insert(name.clone(), v);
                            }
                        }
                    }
                }
            }
            for op in &block.ops {
                match op {
                    MirOp::Phi { .. } => {}
                    MirOp::MatchPayload { .. } => {}
                    MirOp::Set { name, value, .. } => {
                        let v = self.eval(value, env, depth)?;
                        env.insert(name.clone(), v);
                    }
                    MirOp::Eval(e) => {
                        let _ = self.eval(e, env, depth)?;
                    }
                    MirOp::FieldSet { base, field, value } => {
                        let h = self.eval(base, env, depth)?.i64()?;
                        let v = self.eval(value, env, depth)?.i64()?;
                        struct_set_value(h, field, v);
                    }
                    MirOp::IndexSet {
                        base, index, value, ..
                    } => {
                        let h = self.eval(base, env, depth)?.i64()?;
                        let i = self.eval(index, env, depth)?.i64()?;
                        let v = self.eval(value, env, depth)?.i64()?;
                        list_set_value(h, i, v);
                    }
                    MirOp::ListPush { base, value } => {
                        let h = self.eval(base, env, depth)?.i64()?;
                        let v = self.eval(value, env, depth)?.i64()?;
                        list_push_value(h, v);
                    }
                    MirOp::TaskSpawn { .. }
                    | MirOp::TaskSpawnFn { .. }
                    | MirOp::TaskJoin { .. } => {
                        return Err(
                            "playground-host: tasks are not available in the playground".into()
                        );
                    }
                    MirOp::ScopeEnter { id } => {
                        echo_runtime::echo_runtime_scope_enter(i64::from(*id));
                    }
                    MirOp::ScopeExit { id } => {
                        echo_runtime::echo_runtime_scope_exit(i64::from(*id));
                    }
                    MirOp::ScopeRegister { value } => {
                        if let Ok(v) = self.eval(value, env, depth)?.i64() {
                            echo_runtime::echo_runtime_scope_register(v);
                        }
                    }
                    MirOp::ScopePromote { value, target } => {
                        if let Ok(v) = self.eval(value, env, depth)?.i64() {
                            echo_runtime::echo_runtime_scope_promote(v, i64::from(*target));
                        }
                    }
                    MirOp::ScopeDisown { value } => {
                        if let Ok(v) = self.eval(value, env, depth)?.i64() {
                            echo_runtime::echo_runtime_scope_disown(v);
                        }
                    }
                    MirOp::ScopeRelease { value } => {
                        if let Ok(v) = self.eval(value, env, depth)?.i64() {
                            echo_runtime::echo_runtime_scope_release(v);
                        }
                    }
                }
            }
            match &block.term {
                Terminator::Goto(next) => {
                    pred = Some(bb);
                    bb = *next;
                }
                Terminator::Branch {
                    cond,
                    then_bb,
                    else_bb,
                } => {
                    let c = self.eval(cond, env, depth)?.i64()?;
                    pred = Some(bb);
                    bb = if c != 0 { *then_bb } else { *else_bb };
                }
                Terminator::MatchTagged {
                    scrutinee,
                    ok_bb,
                    err_bb,
                } => {
                    let slot = self.eval(scrutinee, env, depth)?;
                    let (tag, payload) = match slot {
                        Slot::Tagged { tag, payload } => (tag, payload),
                        Slot::I64(v) => (0, v),
                        Slot::Fn { symbol, .. } => {
                            return Err(format!("playground-host: cannot match function {symbol}"));
                        }
                    };
                    pred = Some(bb);
                    bb = if tag == 0 { *ok_bb } else { *err_bb };
                    // Apply payload to the successor block's MatchPayload names now.
                    let next = cfg.block(bb);
                    for op in &next.ops {
                        if let MirOp::MatchPayload { name } = op {
                            env.insert(name.clone(), Slot::I64(payload));
                        }
                    }
                }
                Terminator::ReturnOk(e, _) => {
                    let payload = self.eval(e, env, depth)?.i64()?;
                    return Ok(match ret {
                        MirRetShape::Plain => Slot::I64(payload),
                        MirRetShape::Result => Slot::Tagged { tag: 0, payload },
                        MirRetShape::Option => Slot::Tagged { tag: 0, payload },
                    });
                }
                Terminator::ReturnErr(e) => {
                    let payload = self.eval(e, env, depth)?.i64()?;
                    return Ok(Slot::Tagged {
                        tag: TAG_ERR,
                        payload,
                    });
                }
                Terminator::ReturnNone => {
                    return Ok(Slot::Tagged {
                        tag: TAG_NONE,
                        payload: 0,
                    });
                }
                Terminator::Unreachable => {
                    return Ok(Slot::I64(0));
                }
            }
        }
    }

    fn eval(
        &mut self,
        expr: &MirExpr,
        env: &HashMap<String, Slot>,
        depth: u32,
    ) -> Result<Slot, String> {
        match expr {
            MirExpr::ConstI64(n) => Ok(Slot::I64(*n)),
            MirExpr::ConstI32(n) => Ok(Slot::I64(i64::from(*n))),
            MirExpr::ConstInt { value, .. } => Ok(Slot::I64(*value)),
            MirExpr::ConstBool(b) => Ok(Slot::I64(i64::from(*b))),
            MirExpr::ConstF64(f) => Ok(Slot::I64(echo_runtime_float_from_f64(*f))),
            MirExpr::ConstF32(f) => Ok(Slot::I64(echo_runtime_float_from_f64(f64::from(*f)))),
            MirExpr::ConstDuration(n) => Ok(Slot::I64(*n)),
            MirExpr::Cast { expr, .. } => self.eval(expr, env, depth),
            MirExpr::Name(n) => env
                .get(n)
                .cloned()
                .ok_or_else(|| format!("playground-host: unbound {n}")),
            MirExpr::Unary { op, expr } => {
                let v = self.eval(expr, env, depth)?.i64()?;
                Ok(Slot::I64(match op {
                    UnaryOp::Neg => v.wrapping_neg(),
                    UnaryOp::Not => i64::from(v == 0),
                    UnaryOp::BitNot => !v,
                }))
            }
            MirExpr::Binary { op, left, right } => {
                let l = self.eval(left, env, depth)?.i64()?;
                let r = self.eval(right, env, depth)?.i64()?;
                Ok(Slot::I64(eval_binary(*op, l, r)))
            }
            MirExpr::Call { target, args, ret } => self.eval_call(target, args, *ret, env, depth),
            MirExpr::FnValue {
                module_path,
                symbol,
            } => Ok(Slot::Fn {
                module_path: module_path.clone(),
                symbol: symbol.clone(),
            }),
            MirExpr::Range { start, end } => {
                let lo = self.eval(start, env, depth)?.i64()?;
                let hi = self.eval(end, env, depth)?.i64()?;
                Ok(Slot::I64(echo_runtime_range_new(lo, hi)))
            }
            MirExpr::PrimCall { prim, args } => match prim {
                MirPrim::ListLen => {
                    let list = self.eval(&args[0], env, depth)?.i64()?;
                    Ok(Slot::I64(list_len_value(list)))
                }
                MirPrim::ListGetChecked => {
                    let list = self.eval(&args[0], env, depth)?.i64()?;
                    let idx = self.eval(&args[1], env, depth)?.i64()?;
                    Ok(Slot::I64(list_get_value(list, idx)))
                }
            },
            MirExpr::ListLit(elems) => {
                let h = echo_runtime_list_new();
                for e in elems {
                    let v = self.eval(e, env, depth)?.i64()?;
                    list_push_value(h, v);
                }
                Ok(Slot::I64(h))
            }
            MirExpr::StringLit { bytes } => {
                let s = String::from_utf8_lossy(bytes);
                Ok(Slot::I64(string_handle_from_utf8(&s)))
            }
            MirExpr::BytesLit { bytes } => {
                let s = String::from_utf8_lossy(bytes);
                Ok(Slot::I64(string_handle_from_utf8(&s)))
            }
            MirExpr::LocatorLit { text } => Ok(Slot::I64(string_handle_from_utf8(text))),
            MirExpr::LocatorInterp { parts } => {
                let s = eval_string_interp(parts, env)?;
                Ok(Slot::I64(echo_runtime_locator_from_string(s)))
            }
            MirExpr::BytesInterp { parts } => Ok(Slot::I64(eval_bytes_interp(parts, env)?)),
            MirExpr::StringInterp { parts } => Ok(Slot::I64(eval_string_interp(parts, env)?)),
            MirExpr::Index { base, index } => {
                let h = self.eval(base, env, depth)?.i64()?;
                let i = self.eval(index, env, depth)?.i64()?;
                Ok(Slot::I64(list_get_value(h, i)))
            }
            MirExpr::StructLit { type_name, fields } => {
                let h = struct_new_value(type_name);
                for (name, val) in fields {
                    let v = self.eval(val, env, depth)?.i64()?;
                    struct_set_value(h, name, v);
                }
                Ok(Slot::I64(h))
            }
            MirExpr::StructTypeIs { value, type_name } => {
                let h = self.eval(value, env, depth)?.i64()?;
                Ok(Slot::I64(struct_type_is_value(h, type_name)))
            }
            MirExpr::FieldGet { base, field } => {
                let h = self.eval(base, env, depth)?.i64()?;
                Ok(Slot::I64(struct_get_value(h, field)))
            }
            MirExpr::BoxValue { value, .. } | MirExpr::UnboxValue { value, .. } => {
                self.eval(value, env, depth)
            }
        }
    }

    fn eval_call(
        &mut self,
        target: &CallTarget,
        args: &[MirExpr],
        ret: MirRetShape,
        env: &HashMap<String, Slot>,
        depth: u32,
    ) -> Result<Slot, String> {
        if let CallTarget::Indirect { callee } = target {
            let slot = self.eval(callee, env, depth)?;
            let mut vals = Vec::with_capacity(args.len());
            for a in args {
                vals.push(self.eval(a, env, depth)?.i64()?);
            }
            return match slot {
                Slot::Fn {
                    module_path,
                    symbol,
                } => {
                    let func = find_fn(self.program, &module_path, &symbol)?;
                    let called = self.call_fn(func, &vals)?;
                    if ret.is_tagged() {
                        Ok(called)
                    } else {
                        Ok(Slot::I64(called.i64()?))
                    }
                }
                other => Err(format!(
                    "playground-host: indirect call on non-function ({})",
                    other.i64().map(|n| n.to_string()).unwrap_or_else(|e| e)
                )),
            };
        }
        let mut vals = Vec::with_capacity(args.len());
        for a in args {
            vals.push(self.eval(a, env, depth)?.i64()?);
        }
        match target {
            CallTarget::Runtime { export } => {
                if !runtime_export_allowed(export) {
                    return Err(format!(
                        "playground-host: {export} is not available in the playground"
                    ));
                }
                Ok(call_runtime(export, &vals)?)
            }
            CallTarget::Function { module_path, name } => {
                let func = find_fn(self.program, module_path, name)?;
                let slot = self.call_fn(func, &vals)?;
                if ret.is_tagged() {
                    Ok(slot)
                } else {
                    Ok(Slot::I64(slot.i64()?))
                }
            }
            CallTarget::Indirect { callee } => {
                let slot = self.eval(callee, env, depth)?;
                match slot {
                    Slot::Fn {
                        module_path,
                        symbol,
                    } => {
                        let func = find_fn(self.program, &module_path, &symbol)?;
                        let called = self.call_fn(func, &vals)?;
                        if ret.is_tagged() {
                            Ok(called)
                        } else {
                            Ok(Slot::I64(called.i64()?))
                        }
                    }
                    other => Err(format!(
                        "playground-host: indirect call on non-function ({})",
                        other.i64().map(|n| n.to_string()).unwrap_or_else(|e| e)
                    )),
                }
            }
        }
    }
}

fn find_fn<'a>(
    program: &'a MirProgram,
    module_path: &Path,
    name: &str,
) -> Result<&'a MirFn, String> {
    program
        .functions
        .iter()
        .find(|f| f.module_path == module_path && f.name == name)
        .or_else(|| {
            program.functions.iter().find(|f| {
                f.name == name
                    && (f.module_path.parent() == Some(module_path)
                        || f.module_path.starts_with(module_path))
            })
        })
        .ok_or_else(|| format!("playground-host: unknown function {name}"))
}

fn call_runtime(export: &str, args: &[i64]) -> Result<Slot, String> {
    match export {
        "print" => {
            let v = *args.first().ok_or("playground-host: print expects 1 arg")?;
            echo_runtime_print_i64(v);
            Ok(Slot::I64(0))
        }
        "eprint" => {
            let v = *args
                .first()
                .ok_or("playground-host: eprint expects 1 arg")?;
            echo_runtime_eprint(v);
            Ok(Slot::I64(0))
        }
        "str_from_int" => {
            let n = *args
                .first()
                .ok_or("playground-host: str_from_int expects 1 arg")?;
            Ok(Slot::I64(echo_runtime_str_from_int(n)))
        }
        "str_from_debug" => {
            let n = *args
                .first()
                .ok_or("playground-host: str_from_debug expects 1 arg")?;
            Ok(Slot::I64(echo_runtime_str_from_debug(n)))
        }
        "list_len" => {
            let n = *args.first().unwrap_or(&0);
            Ok(Slot::I64(list_len_value(n)))
        }
        "list_get" => Ok(Slot::I64(list_get_value(
            *args.first().unwrap_or(&0),
            *args.get(1).unwrap_or(&0),
        ))),
        other => Err(format!(
            "playground-host: {other} is not available in the playground"
        )),
    }
}

fn eval_binary(op: BinaryOp, l: i64, r: i64) -> i64 {
    match op {
        BinaryOp::Add => l.wrapping_add(r),
        BinaryOp::Sub => l.wrapping_sub(r),
        BinaryOp::Mul => l.wrapping_mul(r),
        BinaryOp::Div => {
            if r == 0 {
                0
            } else {
                l.wrapping_div(r)
            }
        }
        BinaryOp::Rem => {
            if r == 0 {
                0
            } else {
                l.wrapping_rem(r)
            }
        }
        BinaryOp::BitAnd => l & r,
        BinaryOp::BitOr => l | r,
        BinaryOp::BitXor => l ^ r,
        BinaryOp::Shl => l.wrapping_shl((r as u32) & 63),
        BinaryOp::Shr => l.wrapping_shr((r as u32) & 63),
        BinaryOp::Eq | BinaryOp::EqEqEq => i64::from(l == r),
        BinaryOp::NotEq | BinaryOp::NotEqEq => i64::from(l != r),
        BinaryOp::Lt => i64::from(l < r),
        BinaryOp::Gt => i64::from(l > r),
        BinaryOp::LtEq => i64::from(l <= r),
        BinaryOp::GtEq => i64::from(l >= r),
        BinaryOp::And => i64::from(l != 0 && r != 0),
        BinaryOp::Or => i64::from(l != 0 || r != 0),
    }
}
