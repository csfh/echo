//! SSA construction on [`MirCfg`] (Cytron-style φ placement + rename).
//!
//! Locals and params become versioned names `base@ver`. Uses of `Name` are
//! rewritten to the dominating definition. [`MirOp::Phi`] sits at block starts.

use std::collections::{HashMap, HashSet};

use crate::cfg::{BlockId, MirCfg, MirOp, Terminator};
use crate::{CallTarget, MirExpr, StrPart};

/// Build SSA form: insert φ-nodes and rename locals/params.
#[must_use]
pub fn construct_ssa(mut cfg: MirCfg, params: &[String]) -> MirCfg {
    let n = cfg.blocks.len();
    if n == 0 {
        return cfg;
    }

    // Dominance must ignore blocks unreachable from entry. For-in always allocates a
    // continue block with a back-edge to the header; when the body always returns
    // (or otherwise never continues), that cont is dead but still appears as a
    // predecessor of the header. An unreachable pred's dom-set collapses to itself
    // and poisons the header via intersection — φ placement and rename then skip
    // the loop, leaving unversioned uses that codegen treats as 0 (infinite loop).
    let reachable = reachable_from_entry(&cfg);
    let preds = reachable_predecessors(&cfg, &reachable);
    let idom = compute_idom(&cfg, &preds, &reachable);
    let df = dominance_frontiers(&preds, &idom, &reachable);
    let dom_children = dom_tree_children(n, &idom, cfg.entry);

    // base name → blocks that assign it
    let mut def_blocks: HashMap<String, HashSet<BlockId>> = HashMap::new();
    for p in params {
        def_blocks.entry(p.clone()).or_default().insert(cfg.entry);
    }
    for b in &cfg.blocks {
        for op in &b.ops {
            if let Some(name) = def_name(op) {
                def_blocks
                    .entry(base_name(name).to_string())
                    .or_default()
                    .insert(b.id);
            }
        }
    }

    let vars: Vec<String> = def_blocks.keys().cloned().collect();
    for var in &vars {
        let mut worklist: Vec<BlockId> = def_blocks
            .get(var)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default();
        let mut ever_on: HashSet<BlockId> = worklist.iter().copied().collect();
        let mut has_phi: HashSet<BlockId> = HashSet::new();

        while let Some(b) = worklist.pop() {
            for &d in df.get(b.0 as usize).map(|s| s.as_slice()).unwrap_or(&[]) {
                if !has_phi.insert(d) {
                    continue;
                }
                cfg.blocks[d.0 as usize].ops.insert(
                    0,
                    MirOp::Phi {
                        name: var.clone(),
                        incomings: Vec::new(),
                    },
                );
                if ever_on.insert(d) {
                    worklist.push(d);
                }
            }
        }
    }

    let mut stacks: HashMap<String, Vec<String>> = HashMap::new();
    let mut counters: HashMap<String, u32> = HashMap::new();
    for p in params {
        let v = fresh(p, &mut counters);
        stacks.entry(p.clone()).or_default().push(v);
    }

    let mut phi_incomings: Vec<(BlockId, usize, BlockId, String)> = Vec::new();
    let entry = cfg.entry;
    rename_block(
        &mut cfg,
        entry,
        &dom_children,
        &mut stacks,
        &mut counters,
        &mut phi_incomings,
    );
    apply_phi_incomings(&mut cfg, &phi_incomings);
    cfg
}

fn rename_block(
    cfg: &mut MirCfg,
    bid: BlockId,
    dom_children: &[Vec<BlockId>],
    stacks: &mut HashMap<String, Vec<String>>,
    counters: &mut HashMap<String, u32>,
    phi_incomings: &mut Vec<(BlockId, usize, BlockId, String)>,
) {
    let mut pushed: Vec<String> = Vec::new();
    let op_count = cfg.blocks[bid.0 as usize].ops.len();

    for i in 0..op_count {
        // Rewrite uses, then define
        let op = cfg.blocks[bid.0 as usize].ops[i].clone();
        match op {
            MirOp::Phi { name, incomings } => {
                let base = base_name(&name).to_string();
                let v = fresh(&base, counters);
                stacks.entry(base.clone()).or_default().push(v.clone());
                pushed.push(base);
                cfg.blocks[bid.0 as usize].ops[i] = MirOp::Phi {
                    name: v,
                    incomings, // filled later
                };
            }
            MirOp::Set { name, value, span } => {
                let new_val = rewrite_expr(&value, stacks);
                let base = base_name(&name).to_string();
                let v = fresh(&base, counters);
                stacks.entry(base.clone()).or_default().push(v.clone());
                pushed.push(base);
                cfg.blocks[bid.0 as usize].ops[i] = MirOp::Set {
                    name: v,
                    value: new_val,
                    span,
                };
            }
            MirOp::MatchPayload { name } => {
                let base = base_name(&name).to_string();
                let v = fresh(&base, counters);
                stacks.entry(base.clone()).or_default().push(v.clone());
                pushed.push(base);
                cfg.blocks[bid.0 as usize].ops[i] = MirOp::MatchPayload { name: v };
            }
            MirOp::Eval(e) => {
                cfg.blocks[bid.0 as usize].ops[i] = MirOp::Eval(rewrite_expr(&e, stacks));
            }
            MirOp::FieldSet { base, field, value } => {
                cfg.blocks[bid.0 as usize].ops[i] = MirOp::FieldSet {
                    base: rewrite_expr(&base, stacks),
                    field,
                    value: rewrite_expr(&value, stacks),
                };
            }
            MirOp::IndexSet { base, index, value } => {
                cfg.blocks[bid.0 as usize].ops[i] = MirOp::IndexSet {
                    base: rewrite_expr(&base, stacks),
                    index: rewrite_expr(&index, stacks),
                    value: rewrite_expr(&value, stacks),
                };
            }
            MirOp::ListPush { base, value } => {
                cfg.blocks[bid.0 as usize].ops[i] = MirOp::ListPush {
                    base: rewrite_expr(&base, stacks),
                    value: rewrite_expr(&value, stacks),
                };
            }
            MirOp::TaskSpawn {
                module_path,
                body_symbol,
                bind,
            } => {
                let bind = if let Some(name) = bind {
                    let base = base_name(&name).to_string();
                    let v = fresh(&base, counters);
                    stacks.entry(base.clone()).or_default().push(v.clone());
                    pushed.push(base);
                    Some(v)
                } else {
                    None
                };
                cfg.blocks[bid.0 as usize].ops[i] = MirOp::TaskSpawn {
                    module_path,
                    body_symbol,
                    bind,
                };
            }
            MirOp::TaskSpawnFn {
                module_path,
                fn_symbol,
                args,
                bind,
            } => {
                let args: Vec<_> = args.into_iter().map(|a| rewrite_expr(&a, stacks)).collect();
                let bind = if let Some(name) = bind {
                    let base = base_name(&name).to_string();
                    let v = fresh(&base, counters);
                    stacks.entry(base.clone()).or_default().push(v.clone());
                    pushed.push(base);
                    Some(v)
                } else {
                    None
                };
                cfg.blocks[bid.0 as usize].ops[i] = MirOp::TaskSpawnFn {
                    module_path,
                    fn_symbol,
                    args,
                    bind,
                };
            }
            MirOp::TaskJoin {
                module_path,
                body_symbol,
                handle,
                bind,
            } => {
                let handle = handle.map(|h| rewrite_expr(&h, stacks));
                let bind = if let Some(name) = bind {
                    let base = base_name(&name).to_string();
                    let v = fresh(&base, counters);
                    stacks.entry(base.clone()).or_default().push(v.clone());
                    pushed.push(base);
                    Some(v)
                } else {
                    None
                };
                cfg.blocks[bid.0 as usize].ops[i] = MirOp::TaskJoin {
                    module_path,
                    body_symbol,
                    handle,
                    bind,
                };
            }
            MirOp::ScopeEnter { id } => {
                cfg.blocks[bid.0 as usize].ops[i] = MirOp::ScopeEnter { id };
            }
            MirOp::ScopeExit { id } => {
                cfg.blocks[bid.0 as usize].ops[i] = MirOp::ScopeExit { id };
            }
            MirOp::ScopeRegister { value } => {
                cfg.blocks[bid.0 as usize].ops[i] = MirOp::ScopeRegister {
                    value: rewrite_expr(&value, stacks),
                };
            }
            MirOp::ScopePromote { value, target } => {
                cfg.blocks[bid.0 as usize].ops[i] = MirOp::ScopePromote {
                    value: rewrite_expr(&value, stacks),
                    target,
                };
            }
            MirOp::ScopeDisown { value } => {
                cfg.blocks[bid.0 as usize].ops[i] = MirOp::ScopeDisown {
                    value: rewrite_expr(&value, stacks),
                };
            }
            MirOp::ScopeRelease { value } => {
                cfg.blocks[bid.0 as usize].ops[i] = MirOp::ScopeRelease {
                    value: rewrite_expr(&value, stacks),
                };
            }
        }
    }

    let term = cfg.blocks[bid.0 as usize].term.clone();
    cfg.blocks[bid.0 as usize].term = rewrite_term(term, stacks);

    for s in cfg.successors(bid) {
        for (phi_idx, op) in cfg.blocks[s.0 as usize].ops.iter().enumerate() {
            match op {
                MirOp::Phi { name, .. } => {
                    let base = base_name(name).to_string();
                    let ver = stacks
                        .get(&base)
                        .and_then(|st| st.last())
                        .cloned()
                        .unwrap_or_else(|| format!("{base}@undef"));
                    phi_incomings.push((s, phi_idx, bid, ver));
                }
                _ => break,
            }
        }
    }

    for &child in &dom_children[bid.0 as usize] {
        rename_block(cfg, child, dom_children, stacks, counters, phi_incomings);
    }

    for base in pushed.iter().rev() {
        if let Some(st) = stacks.get_mut(base) {
            st.pop();
        }
    }
}

fn apply_phi_incomings(cfg: &mut MirCfg, edges: &[(BlockId, usize, BlockId, String)]) {
    for &(bb, phi_idx, pred, ref ver) in edges {
        if let Some(MirOp::Phi { incomings, .. }) = cfg.blocks[bb.0 as usize].ops.get_mut(phi_idx) {
            if !incomings.iter().any(|(p, _)| *p == pred) {
                incomings.push((pred, ver.clone()));
            }
        }
    }
}

fn def_name(op: &MirOp) -> Option<&str> {
    match op {
        MirOp::Set { name, .. } | MirOp::MatchPayload { name } | MirOp::Phi { name, .. } => {
            Some(name)
        }
        MirOp::TaskSpawn {
            bind: Some(name), ..
        }
        | MirOp::TaskSpawnFn {
            bind: Some(name), ..
        }
        | MirOp::TaskJoin {
            bind: Some(name), ..
        } => Some(name),
        MirOp::Eval(_)
        | MirOp::FieldSet { .. }
        | MirOp::IndexSet { .. }
        | MirOp::ListPush { .. }
        | MirOp::TaskSpawn { bind: None, .. }
        | MirOp::TaskSpawnFn { bind: None, .. }
        | MirOp::TaskJoin { bind: None, .. }
        | MirOp::ScopeEnter { .. }
        | MirOp::ScopeExit { .. }
        | MirOp::ScopeRegister { .. }
        | MirOp::ScopePromote { .. }
        | MirOp::ScopeDisown { .. }
        | MirOp::ScopeRelease { .. } => None,
    }
}

/// Strip SSA version suffix (`x@3` → `x`).
#[must_use]
pub fn base_name(ssa_name: &str) -> &str {
    ssa_name.split('@').next().unwrap_or(ssa_name)
}

fn fresh(base: &str, counters: &mut HashMap<String, u32>) -> String {
    let c = counters.entry(base.to_string()).or_insert(0);
    let v = format!("{base}@{c}");
    *c += 1;
    v
}

fn stack_top(stacks: &HashMap<String, Vec<String>>, name: &str) -> String {
    let base = base_name(name);
    stacks
        .get(base)
        .and_then(|s| s.last())
        .cloned()
        .unwrap_or_else(|| format!("{base}@undef"))
}

fn rewrite_interp_parts(parts: &[StrPart], stacks: &HashMap<String, Vec<String>>) -> Vec<StrPart> {
    parts
        .iter()
        .map(|p| match p {
            StrPart::Lit(b) => StrPart::Lit(b.clone()),
            // `{.field}` is receiver field access — not an SSA local.
            StrPart::Name(n) if n.starts_with('.') => StrPart::Name(n.clone()),
            StrPart::Name(n) => StrPart::Name(stack_top(stacks, n)),
        })
        .collect()
}

fn rewrite_expr(e: &MirExpr, stacks: &HashMap<String, Vec<String>>) -> MirExpr {
    match e {
        MirExpr::Name(n) => MirExpr::Name(stack_top(stacks, n)),
        MirExpr::ConstI64(v) => MirExpr::ConstI64(*v),
        MirExpr::ConstI32(v) => MirExpr::ConstI32(*v),
        MirExpr::ConstInt { value, width } => MirExpr::ConstInt {
            value: *value,
            width: *width,
        },
        MirExpr::Cast { to, expr } => MirExpr::Cast {
            to: *to,
            expr: Box::new(rewrite_expr(expr, stacks)),
        },
        MirExpr::ConstBool(b) => MirExpr::ConstBool(*b),
        MirExpr::ConstF64(v) => MirExpr::ConstF64(*v),
        MirExpr::ConstF32(v) => MirExpr::ConstF32(*v),
        MirExpr::ConstDuration(v) => MirExpr::ConstDuration(*v),
        MirExpr::Unary { op, expr } => MirExpr::Unary {
            op: *op,
            expr: Box::new(rewrite_expr(expr, stacks)),
        },
        MirExpr::Binary { op, left, right } => MirExpr::Binary {
            op: *op,
            left: Box::new(rewrite_expr(left, stacks)),
            right: Box::new(rewrite_expr(right, stacks)),
        },
        MirExpr::Call { target, args, ret } => {
            let target = match target {
                CallTarget::Indirect { callee } => CallTarget::Indirect {
                    callee: Box::new(rewrite_expr(callee, stacks)),
                },
                other => other.clone(),
            };
            MirExpr::Call {
                target,
                args: args.iter().map(|a| rewrite_expr(a, stacks)).collect(),
                ret: *ret,
            }
        }
        MirExpr::FnValue {
            module_path,
            symbol,
        } => MirExpr::FnValue {
            module_path: module_path.clone(),
            symbol: symbol.clone(),
        },
        MirExpr::Range { start, end } => MirExpr::Range {
            start: Box::new(rewrite_expr(start, stacks)),
            end: Box::new(rewrite_expr(end, stacks)),
        },
        MirExpr::PrimCall { prim, args } => MirExpr::PrimCall {
            prim: *prim,
            args: args.iter().map(|a| rewrite_expr(a, stacks)).collect(),
        },
        MirExpr::ListLit(xs) => {
            MirExpr::ListLit(xs.iter().map(|x| rewrite_expr(x, stacks)).collect())
        }
        MirExpr::StringLit { bytes } => MirExpr::StringLit {
            bytes: bytes.clone(),
        },
        MirExpr::BytesLit { bytes } => MirExpr::BytesLit {
            bytes: bytes.clone(),
        },
        MirExpr::LocatorLit { text } => MirExpr::LocatorLit { text: text.clone() },
        MirExpr::StringInterp { parts } => MirExpr::StringInterp {
            parts: rewrite_interp_parts(parts, stacks),
        },
        MirExpr::LocatorInterp { parts } => MirExpr::LocatorInterp {
            parts: rewrite_interp_parts(parts, stacks),
        },
        MirExpr::BytesInterp { parts } => MirExpr::BytesInterp {
            parts: rewrite_interp_parts(parts, stacks),
        },
        MirExpr::Index { base, index } => MirExpr::Index {
            base: Box::new(rewrite_expr(base, stacks)),
            index: Box::new(rewrite_expr(index, stacks)),
        },
        MirExpr::StructLit { type_name, fields } => MirExpr::StructLit {
            type_name: type_name.clone(),
            fields: fields
                .iter()
                .map(|(k, v)| (k.clone(), rewrite_expr(v, stacks)))
                .collect(),
        },
        MirExpr::StructTypeIs { value, type_name } => MirExpr::StructTypeIs {
            value: Box::new(rewrite_expr(value, stacks)),
            type_name: type_name.clone(),
        },
        MirExpr::FieldGet { base, field } => MirExpr::FieldGet {
            base: Box::new(rewrite_expr(base, stacks)),
            field: field.clone(),
        },
        MirExpr::BoxValue { value, from } => MirExpr::BoxValue {
            value: Box::new(rewrite_expr(value, stacks)),
            from: *from,
        },
        MirExpr::UnboxValue { value, to } => MirExpr::UnboxValue {
            value: Box::new(rewrite_expr(value, stacks)),
            to: *to,
        },
    }
}

fn rewrite_term(term: Terminator, stacks: &HashMap<String, Vec<String>>) -> Terminator {
    match term {
        Terminator::Goto(t) => Terminator::Goto(t),
        Terminator::Branch {
            cond,
            then_bb,
            else_bb,
        } => Terminator::Branch {
            cond: rewrite_expr(&cond, stacks),
            then_bb,
            else_bb,
        },
        Terminator::MatchTagged {
            scrutinee,
            ok_bb,
            err_bb,
        } => Terminator::MatchTagged {
            scrutinee: rewrite_expr(&scrutinee, stacks),
            ok_bb,
            err_bb,
        },
        Terminator::ReturnOk(e, span) => Terminator::ReturnOk(rewrite_expr(&e, stacks), span),
        Terminator::ReturnErr(e) => Terminator::ReturnErr(rewrite_expr(&e, stacks)),
        Terminator::ReturnNone => Terminator::ReturnNone,
        Terminator::Unreachable => Terminator::Unreachable,
    }
}

/// Blocks reachable from CFG entry (forward CFG walk).
fn reachable_from_entry(cfg: &MirCfg) -> HashSet<BlockId> {
    let mut seen = HashSet::new();
    let mut stack = vec![cfg.entry];
    while let Some(b) = stack.pop() {
        if !seen.insert(b) {
            continue;
        }
        for s in cfg.successors(b) {
            stack.push(s);
        }
    }
    seen
}

/// Predecessor lists restricted to the reachable subgraph.
fn reachable_predecessors(cfg: &MirCfg, reachable: &HashSet<BlockId>) -> Vec<Vec<BlockId>> {
    let n = cfg.blocks.len();
    let mut preds = vec![Vec::new(); n];
    for b in &cfg.blocks {
        if !reachable.contains(&b.id) {
            continue;
        }
        for s in cfg.successors(b.id) {
            if reachable.contains(&s) {
                preds[s.0 as usize].push(b.id);
            }
        }
    }
    preds
}

fn compute_idom(
    cfg: &MirCfg,
    preds: &[Vec<BlockId>],
    reachable: &HashSet<BlockId>,
) -> Vec<Option<BlockId>> {
    let n = cfg.blocks.len();
    let entry = cfg.entry.0 as usize;
    let mut dom: Vec<HashSet<usize>> = vec![HashSet::new(); n];
    // Unreachable blocks keep empty dom; reachable non-entry start as "all reachable".
    let all_reach: HashSet<usize> = reachable.iter().map(|b| b.0 as usize).collect();
    for b in reachable {
        let bi = b.0 as usize;
        if bi == entry {
            dom[bi] = HashSet::from([entry]);
        } else {
            dom[bi] = all_reach.clone();
        }
    }

    let mut changed = true;
    while changed {
        changed = false;
        for b in reachable {
            let bi = b.0 as usize;
            if bi == entry {
                continue;
            }
            let mut new_dom: Option<HashSet<usize>> = None;
            for p in &preds[bi] {
                let pd = &dom[p.0 as usize];
                new_dom = Some(match new_dom {
                    None => pd.clone(),
                    Some(acc) => acc.intersection(pd).copied().collect(),
                });
            }
            let mut new_dom = new_dom.unwrap_or_default();
            new_dom.insert(bi);
            if new_dom != dom[bi] {
                dom[bi] = new_dom;
                changed = true;
            }
        }
    }

    let mut idom = vec![None; n];
    for b in reachable {
        let bi = b.0 as usize;
        if bi == entry {
            continue;
        }
        let mut strict: HashSet<usize> = dom[bi].clone();
        strict.remove(&bi);
        // Immediate dominator = deepest strict dominator
        let mut best: Option<(usize, usize)> = None;
        for &d in &strict {
            let depth = dom[d].len();
            match best {
                None => best = Some((d, depth)),
                Some((_, bd)) if depth > bd => best = Some((d, depth)),
                _ => {}
            }
        }
        idom[bi] = best.map(|(d, _)| BlockId(d as u32));
    }
    idom
}

fn dominance_frontiers(
    preds: &[Vec<BlockId>],
    idom: &[Option<BlockId>],
    reachable: &HashSet<BlockId>,
) -> Vec<Vec<BlockId>> {
    let n = preds.len();
    let mut df = vec![Vec::new(); n];
    for b in reachable {
        let bi = b.0 as usize;
        if preds[bi].len() < 2 {
            continue;
        }
        for p in &preds[bi] {
            let mut runner = *p;
            let idom_b = idom[bi];
            while Some(runner) != idom_b {
                let r = runner.0 as usize;
                if !df[r].contains(&BlockId(bi as u32)) {
                    df[r].push(BlockId(bi as u32));
                }
                match idom[r] {
                    Some(i) => runner = i,
                    None => break,
                }
            }
        }
    }
    df
}

fn dom_tree_children(n: usize, idom: &[Option<BlockId>], entry: BlockId) -> Vec<Vec<BlockId>> {
    let mut children = vec![Vec::new(); n];
    for b in 0..n {
        if BlockId(b as u32) == entry {
            continue;
        }
        if let Some(p) = idom[b] {
            children[p.0 as usize].push(BlockId(b as u32));
        }
    }
    children
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{structured_to_cfg, MirExpr, MirRetShape, MirStmt};

    #[test]
    fn ssa_renames_straight_line() {
        let stmts = vec![
            MirStmt::Set {
                name: "n".into(),
                value: MirExpr::ConstI64(1),
                span: None,
            },
            MirStmt::Set {
                name: "n".into(),
                value: MirExpr::Binary {
                    op: echo_ast::BinaryOp::Add,
                    left: Box::new(MirExpr::Name("n".into())),
                    right: Box::new(MirExpr::ConstI64(1)),
                },
                span: None,
            },
            MirStmt::ReturnOk(MirExpr::Name("n".into()), None),
        ];
        let cfg = structured_to_cfg(&stmts, MirRetShape::Plain);
        let ssa = construct_ssa(cfg, &[]);
        let sets: Vec<_> = ssa.blocks[0]
            .ops
            .iter()
            .filter_map(|op| match op {
                MirOp::Set { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(sets.len(), 2);
        assert_ne!(sets[0], sets[1]);
        assert!(sets[0].starts_with("n@"));
        match &ssa.blocks[0].term {
            Terminator::ReturnOk(MirExpr::Name(n), _) => assert_eq!(n, sets[1]),
            t => panic!("unexpected term {t:?}"),
        }
        // Second set's RHS uses first version
        match &ssa.blocks[0].ops[1] {
            MirOp::Set {
                value: MirExpr::Binary { left, .. },
                ..
            } => match left.as_ref() {
                MirExpr::Name(n) => assert_eq!(n, sets[0]),
                o => panic!("{o:?}"),
            },
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn ssa_inserts_phi_on_if_merge() {
        let stmts = vec![
            MirStmt::Set {
                name: "x".into(),
                value: MirExpr::ConstI64(0),
                span: None,
            },
            MirStmt::If {
                arms: vec![(
                    MirExpr::ConstI64(1),
                    vec![MirStmt::Set {
                        name: "x".into(),
                        value: MirExpr::ConstI64(1),
                        span: None,
                    }],
                )],
                else_body: Some(vec![MirStmt::Set {
                    name: "x".into(),
                    value: MirExpr::ConstI64(2),
                    span: None,
                }]),
            },
            MirStmt::ReturnOk(MirExpr::Name("x".into()), None),
        ];
        let cfg = structured_to_cfg(&stmts, MirRetShape::Plain);
        let ssa = construct_ssa(cfg, &[]);
        let phi = ssa.blocks.iter().find_map(|b| {
            b.ops.iter().find_map(|op| match op {
                MirOp::Phi { name, incomings } => Some((name.clone(), incomings.clone())),
                _ => None,
            })
        });
        let (name, incs) = phi.expect("expected phi at merge");
        assert!(name.starts_with("x@"), "{name}");
        assert!(incs.len() >= 2, "incomings={incs:?}");
    }

    #[test]
    fn ssa_nested_for_in_with_return_keeps_index_phi() {
        // * chain : buckets { * e : chain { return none } }
        // Inner body always returns → inner cont is dead; must not poison outer SSA.
        let stmts = vec![
            MirStmt::ForIn {
                item: "chain".into(),
                iter: MirExpr::Name("buckets".into()),
                body: vec![MirStmt::ForIn {
                    item: "e".into(),
                    iter: MirExpr::Name("chain".into()),
                    body: vec![MirStmt::ReturnNone],
                }],
            },
            MirStmt::ReturnOk(MirExpr::ConstBool(true), None),
        ];
        let cfg = structured_to_cfg(&stmts, MirRetShape::Option);
        let ssa = construct_ssa(cfg, &["buckets".into()]);
        // Outer loop index must be a versioned name in the header branch (not bare `__i_N`).
        let versioned_index_use = ssa.blocks.iter().any(|b| {
            matches!(
                &b.term,
                Terminator::Branch {
                    cond: MirExpr::Binary { left, .. },
                    ..
                } if matches!(left.as_ref(), MirExpr::Name(n) if n.starts_with("__i_") && n.contains('@'))
            )
        });
        assert!(
            versioned_index_use,
            "for-in header must compare SSA-versioned __i_*; cfg={ssa:?}"
        );
        // Outer loop header needs a φ when the outer cont is reachable (empty chains).
        let outer_i_phi = ssa.blocks.iter().any(|b| {
            b.ops.iter().any(|op| match op {
                MirOp::Phi { name, incomings } => {
                    name.starts_with("__i_") && name.contains('@') && incomings.len() >= 2
                }
                _ => false,
            })
        });
        assert!(
            outer_i_phi,
            "expected multi-incoming versioned __i_* phi for outer loop; cfg={ssa:?}"
        );
    }

    #[test]
    fn ssa_for_in_always_return_still_versions_index() {
        // * e : xs { return none }; single level, empty-list safe path.
        let stmts = vec![
            MirStmt::ForIn {
                item: "e".into(),
                iter: MirExpr::Name("xs".into()),
                body: vec![MirStmt::ReturnNone],
            },
            MirStmt::ReturnOk(MirExpr::ConstBool(true), None),
        ];
        let cfg = structured_to_cfg(&stmts, MirRetShape::Option);
        let ssa = construct_ssa(cfg, &["xs".into()]);
        let versioned = ssa.blocks.iter().any(|b| {
            matches!(
                &b.term,
                Terminator::Branch {
                    cond: MirExpr::Binary { left, .. },
                    ..
                } if matches!(left.as_ref(), MirExpr::Name(n) if n.starts_with("__i_") && n.contains('@'))
            )
        });
        assert!(
            versioned,
            "single for-in with always-return must still rename index; cfg={ssa:?}"
        );
    }

    #[test]
    fn ssa_nested_for_in_after_simplify() {
        use crate::{analyze_reprs, simplify_local};
        let stmts = vec![
            MirStmt::ForIn {
                item: "chain".into(),
                iter: MirExpr::Name("buckets".into()),
                body: vec![MirStmt::ForIn {
                    item: "e".into(),
                    iter: MirExpr::Name("chain".into()),
                    body: vec![MirStmt::ReturnNone],
                }],
            },
            MirStmt::ReturnOk(MirExpr::ConstBool(true), None),
        ];
        let cfg = structured_to_cfg(&stmts, MirRetShape::Option);
        let cfg = construct_ssa(cfg, &["buckets".into()]);
        let (cfg, reprs) = analyze_reprs(cfg, &["buckets".into()]);
        let (cfg, reprs) = simplify_local(cfg, reprs);
        let (cfg, _) = simplify_local(cfg, reprs);
        let versioned = cfg.blocks.iter().any(|b| {
            matches!(
                &b.term,
                Terminator::Branch {
                    cond: MirExpr::Binary { left, .. },
                    ..
                } if matches!(left.as_ref(), MirExpr::Name(n) if n.starts_with("__i_") && n.contains('@'))
            )
        });
        assert!(versioned, "after simplify: cfg={cfg:?}");
    }
}
