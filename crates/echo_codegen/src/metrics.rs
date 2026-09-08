//! Lightweight LLVM IR quality metrics for tests and benches.

/// Counts derived from LLVM IR text (best-effort line scan).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IrMetrics {
    /// Non-empty lines that look like instructions (exclude decls/metadata).
    pub instruction_lines: usize,
    /// `call` instructions (any).
    pub call_count: usize,
    /// Calls into `echo_runtime_*`.
    pub runtime_call_count: usize,
    /// Basic block labels (`name:` at start of line inside a function).
    pub basic_block_count: usize,
    /// `define` functions.
    pub function_count: usize,
    /// Total IR text bytes.
    pub ir_bytes: usize,
}

/// Scan LLVM IR text for coarse quality metrics.
#[must_use]
pub fn measure_ir(ir: &str) -> IrMetrics {
    let mut m = IrMetrics {
        ir_bytes: ir.len(),
        ..Default::default()
    };
    let mut in_fn = false;
    for line in ir.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with(';') {
            continue;
        }
        if t.starts_with("define ") {
            m.function_count += 1;
            in_fn = true;
            continue;
        }
        if t == "}" {
            in_fn = false;
            continue;
        }
        if !in_fn {
            continue;
        }
        // Block label: `name:` or `name: ; preds = ...` without leading `%` assign.
        if t.ends_with(':')
            || (t.contains(':')
                && !t.starts_with('%')
                && !t.starts_with("call")
                && !t.contains('='))
        {
            // e.g. `entry:` or `bb1:                                            ; preds = %...`
            if let Some(before) = t.split_once(':') {
                let name = before.0.trim();
                if !name.is_empty()
                    && !name.contains(' ')
                    && !name.starts_with(';')
                    && !name.contains('=')
                {
                    m.basic_block_count += 1;
                    continue;
                }
            }
        }
        // Instruction-ish lines inside functions.
        if t.starts_with("ret ")
            || t.starts_with("br ")
            || t.starts_with("unreachable")
            || t.starts_with("store ")
            || t.starts_with("call ")
            || t.starts_with("invoke ")
            || t.contains(" = ")
        {
            m.instruction_lines += 1;
        }
        if t.contains("call ") {
            m.call_count += 1;
            if t.contains("@echo_runtime_") {
                m.runtime_call_count += 1;
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_simple_module() {
        let ir = r#"; ModuleID = 'echo'
define i64 @f() {
entry:
  %a = add i64 1, 2
  call void @echo_runtime_print_i64(i64 %a)
  ret i64 0
}
"#;
        let m = measure_ir(ir);
        assert_eq!(m.function_count, 1);
        assert_eq!(m.basic_block_count, 1);
        assert!(m.instruction_lines >= 3);
        assert_eq!(m.runtime_call_count, 1);
        assert!(m.ir_bytes > 0);
    }
}
