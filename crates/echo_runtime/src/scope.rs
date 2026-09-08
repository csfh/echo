//! Scope-owned memory registries (ADR 0016).
//!
//! Every managed heap handle is registered to exactly one owning scope.
//! **Graph promotion** (region evacuation): escape of a root rehomes every
//! reachable allocation still owned by the root's source frame. No tracing GC.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::{
    HEAP_MAGIC, KIND_BYTES, KIND_FLOAT, KIND_FN, KIND_LIST, KIND_LOCATOR, KIND_RANGE, KIND_STRING,
    KIND_STRUCT, header_at, list_elems, struct_fields,
};

/// Logical tombstone: handle may still be in memory until deferred free runs.
#[derive(Default)]
struct ScopeState {
    /// Stack of open scopes (top = innermost).
    stack: Vec<ScopeFrame>,
    /// handle → stack index of the owning frame (exactly one owner when live).
    /// Indices are invalidated on pop; exit path frees via the popped frame set.
    owner: HashMap<i64, usize>,
    /// Logically released; physical free may be deferred.
    dead: HashSet<i64>,
    /// Deferred physical frees (event-loop batching).
    deferred: Vec<i64>,
    /// Monotonic epoch for graph-promote visit marks (header.promotion_epoch).
    promote_epoch: u32,
}

struct ScopeFrame {
    id: u32,
    owned: HashSet<i64>,
}

thread_local! {
    static STATE: RefCell<ScopeState> = RefCell::new(ScopeState::default());
}

/// Enter a scope (push). Compile-time `id`s are per-function; the same id may
/// re-enter across nested calls (each push is a distinct dynamic frame).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_scope_enter(id: i64) {
    let id = id as u32;
    STATE.with(|c| {
        let mut st = c.borrow_mut();
        st.stack.push(ScopeFrame {
            id,
            owned: HashSet::new(),
        });
    });
}

/// Register `handle` as owned by the **current** (innermost) scope.
/// No-op for 0 / non-heap immediates.
/// If already owned, leave ownership unchanged (alias-safe; no double free).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_scope_register(handle: i64) {
    if handle == 0 || !is_managed_handle(handle) {
        return;
    }
    STATE.with(|c| {
        let mut st = c.borrow_mut();
        // Address reuse after free: drop tombstone and re-home as a new live object.
        st.dead.remove(&handle);
        if st.owner.contains_key(&handle) {
            // Already owned (alias or re-register) — keep single owner.
            return;
        }
        let Some(idx) = st.stack.len().checked_sub(1) else {
            // No open scope: process root — leave unregistered (slice-1 safety).
            return;
        };
        st.stack[idx].owned.insert(handle);
        st.owner.insert(handle, idx);
    });
}

/// Move ownership of `handle` **and its managed graph** to open scope `target_id`.
///
/// **Graph promotion (region evacuation):** source = current owner of `handle`.
/// Every reachable managed allocation still owned by that source frame is
/// rehomed to `target_id`. Allocations owned by other frames (e.g. longer-lived
/// shared roots) are left unchanged. Cycles are safe via header promotion epoch.
///
/// When the same compile-time id is nested (re-entrant calls), promotion targets
/// the **innermost** open frame with that id.
///
/// Unowned root: rehome root only to target (no walk).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_scope_promote(handle: i64, target_id: i64) {
    echo_runtime_scope_promote_graph(handle, target_id);
}

/// Explicit graph promote (same semantics as [`echo_runtime_scope_promote`]).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_scope_promote_graph(handle: i64, target_id: i64) {
    if handle == 0 || !is_managed_handle(handle) {
        return;
    }
    let target_id = target_id as u32;
    STATE.with(|c| {
        let mut st = c.borrow_mut();
        if st.dead.contains(&handle) {
            st.dead.remove(&handle);
        }
        let Some(target_idx) = st.stack.iter().rposition(|f| f.id == target_id) else {
            eprintln!("echo: promote to non-open scope {target_id}");
            std::process::exit(1);
        };

        let Some(&source_idx) = st.owner.get(&handle) else {
            // Unowned root: rehome root only; do not invent a source for children.
            rehome(&mut st, handle, target_idx);
            return;
        };
        if source_idx == target_idx {
            return;
        }
        if source_idx >= st.stack.len() {
            rehome(&mut st, handle, target_idx);
            return;
        }

        // New promotion epoch (never 0 after first promote).
        let epoch = st.promote_epoch.wrapping_add(1).max(1);
        st.promote_epoch = epoch;

        let mut queue = VecDeque::new();
        if mark_epoch(handle, epoch) {
            queue.push_back(handle);
        }

        while let Some(h) = queue.pop_front() {
            // Only evacuate allocations still owned by the source frame.
            match st.owner.get(&h).copied() {
                Some(idx) if idx == source_idx => {}
                _ => continue,
            }
            rehome(&mut st, h, target_idx);
            for child in managed_children(h) {
                if !is_managed_handle(child) {
                    continue;
                }
                if mark_epoch(child, epoch) {
                    queue.push_back(child);
                }
            }
        }
    });
}

/// Rehome a single handle to `target_idx` (must be valid stack index).
fn rehome(st: &mut ScopeState, handle: i64, target_idx: usize) {
    if let Some(&src_idx) = st.owner.get(&handle) {
        if src_idx == target_idx {
            return;
        }
        if src_idx < st.stack.len() {
            st.stack[src_idx].owned.remove(&handle);
        }
    } else {
        for f in st.stack.iter_mut() {
            f.owned.remove(&handle);
        }
    }
    if target_idx < st.stack.len() {
        st.stack[target_idx].owned.insert(handle);
        st.owner.insert(handle, target_idx);
    }
}

/// Mark header promotion_epoch; returns true if newly marked for this epoch.
fn mark_epoch(handle: i64, epoch: u32) -> bool {
    let Some(h) = (unsafe { header_at(handle) }) else {
        return false;
    };
    // Safety: live heap header from our allocator.
    let hdr = h as *mut crate::HeapHeader;
    unsafe {
        if (*hdr).promotion_epoch == epoch {
            return false;
        }
        (*hdr).promotion_epoch = epoch;
    }
    true
}

/// Managed children of a heap value (list elems / struct fields that are live heap).
fn managed_children(handle: i64) -> Vec<i64> {
    let Some(h) = (unsafe { header_at(handle) }) else {
        return Vec::new();
    };
    let kind = unsafe { (*h).kind };
    match kind {
        KIND_LIST => list_elems(handle)
            .unwrap_or_default()
            .into_iter()
            .filter(|&e| is_managed_handle(e))
            .collect(),
        KIND_STRUCT => struct_fields(handle)
            .unwrap_or_default()
            .into_iter()
            .map(|(_, v)| v)
            .filter(|&e| is_managed_handle(e))
            .collect(),
        KIND_STRING | KIND_BYTES | KIND_FLOAT | KIND_LOCATOR | KIND_RANGE | KIND_FN | _ => {
            Vec::new()
        }
    }
}

/// Remove ownership without free (e.g. return / transfer out of analysis).
///
/// **Graph disown:** the handle and every managed child reachable through list
/// elements / struct fields leave ownership together. Returning a nest like
/// `holder = [xs]` must not free `xs` on the subsequent scope exit while
/// `holder` escapes.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_scope_disown(handle: i64) {
    if handle == 0 {
        return;
    }
    STATE.with(|c| {
        let mut st = c.borrow_mut();
        if !is_managed_handle(handle) {
            // Bare integers / non-heap: no-op (scalar return temps).
            return;
        }
        let mut queue = VecDeque::new();
        queue.push_back(handle);
        let mut seen = HashSet::new();
        while let Some(h) = queue.pop_front() {
            if !seen.insert(h) {
                continue;
            }
            disown_one(&mut st, h);
            for child in managed_children(h) {
                if is_managed_handle(child) {
                    queue.push_back(child);
                }
            }
        }
    });
}

fn disown_one(st: &mut ScopeState, handle: i64) {
    if let Some(idx) = st.owner.remove(&handle) {
        if idx < st.stack.len() {
            st.stack[idx].owned.remove(&handle);
        }
    } else {
        for f in st.stack.iter_mut() {
            f.owned.remove(&handle);
        }
    }
}

/// Logical release of one value (immediate lightweight free when possible).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_scope_release(handle: i64) {
    if handle == 0 {
        return;
    }
    STATE.with(|c| {
        let mut st = c.borrow_mut();
        logical_release(&mut st, handle, /*defer_heavy*/ false);
    });
}

/// Enqueue physical destruction (batch drain later).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_scope_enqueue_release(handle: i64) {
    if handle == 0 {
        return;
    }
    STATE.with(|c| {
        let mut st = c.borrow_mut();
        logical_release(&mut st, handle, /*defer_heavy*/ true);
    });
}

/// Exit scope `id` (must be the current top). Releases every still-owned value.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_scope_exit(id: i64) {
    let id = id as u32;
    STATE.with(|c| {
        let mut st = c.borrow_mut();
        let Some(frame) = st.stack.pop() else {
            eprintln!("echo: scope_exit with empty stack");
            std::process::exit(1);
        };
        if frame.id != id {
            eprintln!(
                "echo: scope_exit id mismatch: expected {}, got {id}",
                frame.id
            );
            std::process::exit(1);
        }
        // Release in reverse insertion order approximation: iterate owned set.
        let owned: Vec<i64> = frame.owned.into_iter().collect();
        for h in owned {
            logical_release(&mut st, h, /*defer_heavy*/ false);
        }
    });
}

/// Drain deferred physical frees (call from event-loop idle points).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_scope_drain_deferred() {
    STATE.with(|c| {
        let mut st = c.borrow_mut();
        let batch = std::mem::take(&mut st.deferred);
        for h in batch {
            physical_free(h);
        }
    });
}

fn logical_release(st: &mut ScopeState, handle: i64, defer_heavy: bool) {
    if handle == 0 {
        return;
    }
    if !st.dead.insert(handle) {
        // Already logically dead — ignore (alias-safe).
        return;
    }
    st.owner.remove(&handle);
    // Scrub from every open frame (safe after scope_exit already popped its frame).
    for f in st.stack.iter_mut() {
        f.owned.remove(&handle);
    }
    // Slice-2: immediate physical free on scope exit / release. Enqueue only when
    // callers request batching (`enqueue_release` / defer_heavy). Promote/exit
    // coverage is precise enough that process-lived deferral is no longer the
    // steady state for values whose ownership ended.
    if defer_heavy {
        st.deferred.push(handle);
    } else {
        physical_free(handle);
    }
}

fn is_managed_handle(handle: i64) -> bool {
    // Prefer the live-set: never probe wild integers (ui64 digests, etc.) as
    // pointers — that segfaults in header_at.
    if crate::is_live_heap(handle) {
        return true;
    }
    false
}

/// Physical free by kind. Safe only after logical release.
fn physical_free(handle: i64) {
    if handle == 0 {
        return;
    }
    if !crate::is_live_heap(handle) {
        return;
    }
    let Some(h) = (unsafe { header_at(handle) }) else {
        crate::note_heap_free(handle);
        return;
    };
    let kind = unsafe { (*h).kind };
    // Reconstruct Box and drop.
    match kind {
        KIND_LIST => {
            let _ = unsafe { Box::from_raw(handle as *mut crate::EchoList) };
        }
        KIND_STRING => {
            let _ = unsafe { Box::from_raw(handle as *mut crate::EchoString) };
        }
        KIND_STRUCT => {
            let _ = unsafe { Box::from_raw(handle as *mut crate::EchoStruct) };
        }
        KIND_FLOAT => {
            let _ = unsafe { Box::from_raw(handle as *mut crate::EchoFloat) };
        }
        KIND_BYTES => {
            let _ = unsafe { Box::from_raw(handle as *mut crate::EchoBytes) };
        }
        KIND_LOCATOR => {
            let _ = unsafe { Box::from_raw(handle as *mut crate::EchoLocator) };
        }
        KIND_RANGE => {
            let _ = unsafe { Box::from_raw(handle as *mut crate::EchoRange) };
        }
        KIND_FN => {
            let _ = unsafe { Box::from_raw(handle as *mut crate::EchoFn) };
        }
        // TLS listener / stream (kinds defined in `tls` module).
        #[cfg(feature = "host-io")]
        20 | 21 => unsafe {
            crate::tls::free_tls_object(handle as *mut u8, kind);
        },
        #[cfg(feature = "host-io")]
        crate::net::KIND_TCP_LISTENER
        | crate::net::KIND_TCP_STREAM
        | crate::net::KIND_UDP_SOCKET => {
            crate::net::free_net_object(handle, kind);
        }
        #[cfg(feature = "host-io")]
        crate::task::KIND_TASK => {
            crate::task::free_task_object(handle);
        }
        #[cfg(feature = "host-io")]
        crate::fs::KIND_FS_FILE => {
            crate::fs::free_file_object(handle);
        }
        _ => {
            return;
        }
    }
    crate::note_heap_free(handle);
    let _ = HEAP_MAGIC; // keep magic used
}

/// Test helper: number of open scopes.
#[cfg(test)]
pub fn test_open_scope_count() -> usize {
    STATE.with(|c| c.borrow().stack.len())
}

/// Test helper: whether handle is owned.
#[cfg(test)]
pub fn test_is_owned(handle: i64) -> bool {
    STATE.with(|c| c.borrow().owner.contains_key(&handle))
}

/// Test helper: compile-time scope id of the frame that owns `handle`, if any.
#[cfg(test)]
pub fn test_owner_scope_id(handle: i64) -> Option<u32> {
    STATE.with(|c| {
        let st = c.borrow();
        st.owner
            .get(&handle)
            .and_then(|&idx| st.stack.get(idx).map(|f| f.id))
    })
}

/// Test helper: clear all scope state (tests only).
#[cfg(test)]
pub fn test_reset() {
    STATE.with(|c| {
        let mut st = c.borrow_mut();
        // Free remaining without process exit.
        let owned: Vec<i64> = st.owner.keys().copied().collect();
        for h in owned {
            if st.dead.insert(h) {
                physical_free(h);
            }
        }
        st.stack.clear();
        st.owner.clear();
        st.dead.clear();
        st.promote_epoch = 0;
        let def = std::mem::take(&mut st.deferred);
        for h in def {
            physical_free(h);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::echo_runtime_list_new;

    #[test]
    fn enter_register_exit_releases() {
        test_reset();
        echo_runtime_scope_enter(1);
        let h = echo_runtime_list_new();
        echo_runtime_scope_register(h);
        assert!(test_is_owned(h));
        echo_runtime_scope_exit(1);
        assert!(!test_is_owned(h));
        assert_eq!(test_open_scope_count(), 0);
        test_reset();
    }

    #[test]
    fn promote_moves_owner() {
        test_reset();
        echo_runtime_scope_enter(1); // outer
        echo_runtime_scope_enter(2); // inner
        let h = echo_runtime_list_new();
        echo_runtime_scope_register(h);
        echo_runtime_scope_promote(h, 1);
        echo_runtime_scope_exit(2); // must not free h
        assert!(test_is_owned(h));
        echo_runtime_scope_exit(1);
        assert!(!test_is_owned(h));
        test_reset();
    }

    #[test]
    fn reentrant_root_scope_ids() {
        test_reset();
        echo_runtime_scope_enter(0);
        let outer = echo_runtime_list_new();
        echo_runtime_scope_register(outer);
        echo_runtime_scope_enter(0); // nested call frame, same compile-time id
        let inner = echo_runtime_list_new();
        echo_runtime_scope_register(inner);
        echo_runtime_scope_exit(0); // frees inner only
        assert!(!test_is_owned(inner));
        assert!(test_is_owned(outer));
        echo_runtime_scope_exit(0);
        assert!(!test_is_owned(outer));
        test_reset();
    }

    #[test]
    fn double_release_is_idempotent() {
        test_reset();
        echo_runtime_scope_enter(1);
        let h = echo_runtime_list_new();
        echo_runtime_scope_register(h);
        echo_runtime_scope_release(h);
        echo_runtime_scope_release(h); // no crash
        echo_runtime_scope_exit(1);
        test_reset();
    }

    #[test]
    fn disown_graph_keeps_nested_child_live_across_exit() {
        use crate::echo_runtime_list_push;
        test_reset();
        echo_runtime_scope_enter(1);
        let child = echo_runtime_list_new();
        echo_runtime_scope_register(child);
        let holder = echo_runtime_list_new();
        unsafe {
            echo_runtime_list_push(holder, child);
        }
        echo_runtime_scope_register(holder);
        // Return nest: disown holder must also drop ownership of child.
        echo_runtime_scope_disown(holder);
        assert!(!test_is_owned(holder));
        assert!(
            !test_is_owned(child),
            "child must leave ownership with graph disown"
        );
        echo_runtime_scope_exit(1);
        // Handles must still be readable (not physically freed).
        assert_eq!(unsafe { crate::echo_runtime_list_len(holder) }, 1);
        assert_eq!(unsafe { crate::echo_runtime_list_get(holder, 0) }, child);
        assert_eq!(unsafe { crate::echo_runtime_list_len(child) }, 0);
        // Manual free for test cleanup (not scope-owned).
        physical_free(child);
        physical_free(holder);
        test_reset();
    }

    #[test]
    fn exit_physically_frees_handle() {
        test_reset();
        echo_runtime_scope_enter(1);
        let h = echo_runtime_list_new();
        echo_runtime_scope_register(h);
        assert!(crate::is_live_heap(h));
        echo_runtime_scope_exit(1);
        assert!(!test_is_owned(h));
        assert!(
            !crate::is_live_heap(h),
            "scope exit must physically free (slice 2)"
        );
        assert_eq!(
            STATE.with(|c| c.borrow().deferred.len()),
            0,
            "exit path must not leave forever-deferred frees"
        );
        test_reset();
    }

    #[test]
    fn enqueue_then_drain_physically_frees() {
        test_reset();
        echo_runtime_scope_enter(1);
        let h = echo_runtime_list_new();
        echo_runtime_scope_register(h);
        echo_runtime_scope_enqueue_release(h);
        assert!(!test_is_owned(h));
        assert!(
            crate::is_live_heap(h),
            "enqueue defers physical free until drain"
        );
        echo_runtime_scope_drain_deferred();
        assert!(!crate::is_live_heap(h));
        echo_runtime_scope_exit(1);
        test_reset();
    }

    #[test]
    fn promote_survives_inner_exit_then_outer_frees() {
        test_reset();
        echo_runtime_scope_enter(1);
        echo_runtime_scope_enter(2);
        let h = echo_runtime_list_new();
        echo_runtime_scope_register(h);
        echo_runtime_scope_promote(h, 1);
        echo_runtime_scope_exit(2);
        assert!(test_is_owned(h));
        assert!(
            crate::is_live_heap(h),
            "promoted value must survive inner exit"
        );
        echo_runtime_scope_exit(1);
        assert!(!crate::is_live_heap(h));
        test_reset();
    }

    /// Deterministic graph promote: nest — both a and b leave inner; outer free kills both.
    #[test]
    fn graph_promote_nested_struct_child() {
        use crate::{echo_runtime_struct_new, struct_set_str};
        test_reset();
        echo_runtime_scope_enter(0); // outer T
        echo_runtime_scope_enter(1); // source S
        let b = echo_runtime_list_new();
        echo_runtime_scope_register(b);
        let a = echo_runtime_struct_new();
        echo_runtime_scope_register(a);
        unsafe {
            struct_set_str(a, "child", b);
        }
        assert_eq!(test_owner_scope_id(a), Some(1));
        assert_eq!(test_owner_scope_id(b), Some(1));

        echo_runtime_scope_promote_graph(a, 0);

        assert_eq!(test_owner_scope_id(a), Some(0), "root rehomed to outer");
        assert_eq!(
            test_owner_scope_id(b),
            Some(0),
            "child owned by S must follow graph promote"
        );
        assert!(crate::is_live_heap(a) && crate::is_live_heap(b));

        echo_runtime_scope_exit(1); // must not free a or b
        assert!(crate::is_live_heap(a));
        assert!(crate::is_live_heap(b));
        assert_eq!(test_owner_scope_id(a), Some(0));
        assert_eq!(test_owner_scope_id(b), Some(0));

        echo_runtime_scope_exit(0);
        assert!(!crate::is_live_heap(a));
        assert!(!crate::is_live_heap(b));
        test_reset();
    }

    /// Cycle a↔b both owned by S; promote terminates; both at T; exit S safe.
    #[test]
    fn graph_promote_cycle_terminates() {
        use crate::{echo_runtime_struct_new, struct_set_str};
        test_reset();
        echo_runtime_scope_enter(0);
        echo_runtime_scope_enter(1);
        let a = echo_runtime_struct_new();
        let b = echo_runtime_struct_new();
        echo_runtime_scope_register(a);
        echo_runtime_scope_register(b);
        unsafe {
            struct_set_str(a, "to", b);
            struct_set_str(b, "to", a);
        }
        echo_runtime_scope_promote_graph(a, 0);
        assert_eq!(test_owner_scope_id(a), Some(0));
        assert_eq!(test_owner_scope_id(b), Some(0));
        echo_runtime_scope_exit(1);
        assert!(crate::is_live_heap(a) && crate::is_live_heap(b));
        echo_runtime_scope_exit(0);
        assert!(!crate::is_live_heap(a) && !crate::is_live_heap(b));
        test_reset();
    }

    /// Shared outer must not be stolen when promoting a child of S.
    #[test]
    fn graph_promote_leaves_longer_lived_shared() {
        use crate::{echo_runtime_struct_new, struct_set_str};
        test_reset();
        echo_runtime_scope_enter(0); // outer
        let shared = echo_runtime_list_new();
        echo_runtime_scope_register(shared);
        assert_eq!(test_owner_scope_id(shared), Some(0));

        echo_runtime_scope_enter(1); // S
        let a = echo_runtime_struct_new();
        echo_runtime_scope_register(a);
        unsafe {
            struct_set_str(a, "parent", shared);
        }
        echo_runtime_scope_promote_graph(a, 0);

        assert_eq!(test_owner_scope_id(a), Some(0));
        assert_eq!(
            test_owner_scope_id(shared),
            Some(0),
            "shared already outer — still outer (not double-owned)"
        );
        // shared was already at 0; promote must not free or re-register wrongly
        assert!(crate::is_live_heap(shared));

        echo_runtime_scope_exit(1);
        assert!(crate::is_live_heap(a));
        assert!(crate::is_live_heap(shared));
        echo_runtime_scope_exit(0);
        assert!(!crate::is_live_heap(a));
        assert!(!crate::is_live_heap(shared));
        test_reset();
    }

    /// List nest: holder=[xs] graph promote from inner moves both.
    #[test]
    fn graph_promote_list_element() {
        use crate::echo_runtime_list_push;
        test_reset();
        echo_runtime_scope_enter(0);
        echo_runtime_scope_enter(1);
        let xs = echo_runtime_list_new();
        echo_runtime_scope_register(xs);
        unsafe {
            echo_runtime_list_push(xs, 7);
        }
        let holder = echo_runtime_list_new();
        echo_runtime_scope_register(holder);
        unsafe {
            echo_runtime_list_push(holder, xs);
        }
        echo_runtime_scope_promote_graph(holder, 0);
        assert_eq!(test_owner_scope_id(holder), Some(0));
        assert_eq!(test_owner_scope_id(xs), Some(0));
        echo_runtime_scope_exit(1);
        assert!(crate::is_live_heap(holder) && crate::is_live_heap(xs));
        echo_runtime_scope_exit(0);
        assert!(!crate::is_live_heap(holder) && !crate::is_live_heap(xs));
        test_reset();
    }

    /// Running the same promote sequence twice yields the same ownership outcomes.
    #[test]
    fn graph_promote_deterministic_twice() {
        use crate::{echo_runtime_struct_new, struct_set_str};
        fn once() -> (bool, bool, u32, u32) {
            test_reset();
            echo_runtime_scope_enter(0);
            echo_runtime_scope_enter(1);
            let b = echo_runtime_list_new();
            echo_runtime_scope_register(b);
            let a = echo_runtime_struct_new();
            echo_runtime_scope_register(a);
            unsafe {
                struct_set_str(a, "child", b);
            }
            echo_runtime_scope_promote_graph(a, 0);
            echo_runtime_scope_exit(1);
            let live_a = crate::is_live_heap(a);
            let live_b = crate::is_live_heap(b);
            let oa = test_owner_scope_id(a).unwrap_or(u32::MAX);
            let ob = test_owner_scope_id(b).unwrap_or(u32::MAX);
            echo_runtime_scope_exit(0);
            test_reset();
            (live_a, live_b, oa, ob)
        }
        let r1 = once();
        let r2 = once();
        assert_eq!(r1, r2, "graph promote outcomes must be deterministic");
        assert_eq!(r1, (true, true, 0, 0));
    }

    fn str_handle(text: &str) -> i64 {
        unsafe { crate::echo_runtime_string_from_utf8(text.as_ptr(), text.len()) }
    }

    #[test]
    fn exit_frees_task_tcp_udp_fs_handles() {
        test_reset();
        echo_runtime_scope_enter(1);

        let task = crate::task::test_alloc_task_handle();
        echo_runtime_scope_register(task);
        assert!(crate::is_live_heap(task));

        let addr = str_handle("127.0.0.1:0");
        let tcp = unsafe { crate::echo_runtime_tcp_listen(addr) };
        assert_ne!(tcp, 0, "tcp listen");
        echo_runtime_scope_register(tcp);

        let udp = unsafe { crate::echo_runtime_udp_bind(addr) };
        assert_ne!(udp, 0, "udp bind");
        echo_runtime_scope_register(udp);

        let tmp = std::env::temp_dir().join(format!(
            "echo_scope_fs_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let path = str_handle(tmp.to_str().unwrap());
        let file = crate::echo_runtime_fs_open_write(path);
        assert_ne!(file, 0, "fs open_write");
        echo_runtime_scope_register(file);

        echo_runtime_scope_exit(1);
        assert!(!crate::is_live_heap(task), "task handle must be freed");
        assert!(!crate::is_live_heap(tcp), "tcp listener must be freed");
        assert!(!crate::is_live_heap(udp), "udp socket must be freed");
        assert!(!crate::is_live_heap(file), "fs file must be freed");
        let _ = std::fs::remove_file(&tmp);
        test_reset();
    }
}
