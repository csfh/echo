//! Piped `xo repl` form coverage — drives the real binary entry (non-TTY stdin).

use std::io::Write;
use std::process::{Command, Stdio};

fn xo_bin() -> &'static str {
    env!("CARGO_BIN_EXE_xo")
}

fn run_repl(input: &str) -> (i32, String, String) {
    let mut child = Command::new(xo_bin())
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn xo repl");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin
            .write_all(input.as_bytes())
            .expect("write stdin");
    }
    let out = child.wait_with_output().expect("wait xo repl");
    let code = out.status.code().unwrap_or(-1);
    (
        code,
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn piped_bind_and_bare_int() {
    let (code, stdout, stderr) = run_repl("$ x = 40\nx + 2\n:quit\n");
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.contains("42") || stdout.trim() == "42", "stdout={stdout:?}");
    assert!(!stderr.contains("Segmentation fault"), "{stderr}");
}

#[test]
fn piped_function_struct_list_import() {
    let script = r#"
$ add = (a, b) {
    ^ a + b
}
add(20, 22)
% point {
    ~ x
    ~ y
}
$ p = point { x: 3, y: 4 }
p.x
$ xs = [1, 2, 3]
~ sum = 0
* x : xs {
    ~ sum = sum + x
}
sum
/ std/io
io.print("hello")
:quit
"#;
    let (code, stdout, stderr) = run_repl(script);
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.contains("42"), "stdout={stdout:?}");
    assert!(stdout.contains('3'), "stdout={stdout:?}");
    assert!(stdout.contains('6'), "stdout={stdout:?}");
    assert!(stdout.contains("hello"), "stdout={stdout:?}");
}

#[test]
fn piped_result_match_and_task_join() {
    let script = r#"
/ std/io
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
+ job = {
    ^ 7
}
- v = job
v
$ x = 1
x + v
:quit
"#;
    let (code, stdout, stderr) = run_repl(script);
    assert_eq!(code, 0, "stderr={stderr}");
    // result arm print + joined v + later x+v
    assert!(stdout.contains('7'), "stdout={stdout:?}");
    assert!(stdout.contains('8'), "stdout={stdout:?}");
    // Unjoined warning may appear for the spawn-only intermediate eval; must not crash.
    assert!(
        !stderr.to_lowercase().contains("segmentation"),
        "stderr={stderr}"
    );
}
