//! Filesystem: paths, files, directories, metadata, streaming handles (`std/fs`).
//!
//! Path arguments accept **string** or **locator** handles. File payloads are
//! **bytes** (string inputs to write are taken as UTF-8 bytes).

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    bytes_data, bytes_to_handle, echo_runtime_list_new, echo_runtime_list_push, header_at,
    locator_data, string_data, string_to_handle, HEAP_MAGIC, HeapHeader,
};

/// Opaque open file (streaming). Distinct from path-based `fs_read` / `fs_write`.
pub(crate) const KIND_FS_FILE: u32 = 14;

#[repr(C)]
struct EchoFile {
    header: HeapHeader,
    inner: Option<File>,
}

fn file_header() -> HeapHeader {
    HeapHeader {
        magic: HEAP_MAGIC,
        kind: KIND_FS_FILE,
        promotion_epoch: 0,
    }
}

fn file_to_handle(f: File) -> i64 {
    let box_f = Box::new(EchoFile {
        header: file_header(),
        inner: Some(f),
    });
    crate::heap_to_handle(box_f)
}

fn file_mut(handle: i64) -> Option<&'static mut EchoFile> {
    if handle == 0 {
        return None;
    }
    let h = unsafe { header_at(handle)? };
    if unsafe { (*h).magic } != HEAP_MAGIC || unsafe { (*h).kind } != KIND_FS_FILE {
        return None;
    }
    Some(unsafe { &mut *(handle as *mut EchoFile) })
}

fn path_from_handle(v: i64) -> Option<PathBuf> {
    if let Some(s) = string_data(v) {
        return Some(PathBuf::from(s));
    }
    if let Some(s) = locator_data(v) {
        return Some(PathBuf::from(s));
    }
    None
}

fn payload_bytes(v: i64) -> Option<Vec<u8>> {
    if let Some(b) = bytes_data(v) {
        return Some(b);
    }
    if let Some(s) = string_data(v) {
        return Some(s.into_bytes());
    }
    None
}

/// 1 if path exists, else 0.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_exists(path: i64) -> i64 {
    match path_from_handle(path) {
        Some(p) if p.exists() => 1,
        _ => 0,
    }
}

/// 1 if path is a regular file, else 0.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_is_file(path: i64) -> i64 {
    match path_from_handle(path) {
        Some(p) if p.is_file() => 1,
        _ => 0,
    }
}

/// 1 if path is a directory, else 0.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_is_dir(path: i64) -> i64 {
    match path_from_handle(path) {
        Some(p) if p.is_dir() => 1,
        _ => 0,
    }
}

/// Join two path components → string handle. Empty string on bad handles.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_join(base: i64, rel: i64) -> i64 {
    let Some(b) = path_from_handle(base) else {
        return string_to_handle(String::new());
    };
    let Some(r) = path_from_handle(rel) else {
        return string_to_handle(String::new());
    };
    let joined = b.join(r);
    string_to_handle(joined.to_string_lossy().into_owned())
}

/// Read file as bytes handle. **0** on failure (missing path, I/O error).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_read(path: i64) -> i64 {
    let Some(p) = path_from_handle(path) else {
        return 0;
    };
    match fs::read(&p) {
        Ok(data) => bytes_to_handle(data),
        Err(_) => 0,
    }
}

/// Write bytes/string payload to path (create/truncate). **0** ok, **-1** fail.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_write(path: i64, data: i64) -> i64 {
    let Some(p) = path_from_handle(path) else {
        return -1;
    };
    let Some(bytes) = payload_bytes(data) else {
        return -1;
    };
    match fs::write(&p, bytes) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Remove a file. **0** ok, **-1** fail.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_remove(path: i64) -> i64 {
    let Some(p) = path_from_handle(path) else {
        return -1;
    };
    match fs::remove_file(&p) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Create a single directory (parent must exist). **0** ok, **-1** fail.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_create_dir(path: i64) -> i64 {
    let Some(p) = path_from_handle(path) else {
        return -1;
    };
    match fs::create_dir(&p) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Create directory and parents. **0** ok, **-1** fail.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_create_dir_all(path: i64) -> i64 {
    let Some(p) = path_from_handle(path) else {
        return -1;
    };
    match fs::create_dir_all(&p) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// List directory entry names (not full paths) as list of strings.
/// **0** on failure. Does not include `.` / `..`.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_read_dir(path: i64) -> i64 {
    let Some(p) = path_from_handle(path) else {
        return 0;
    };
    let rd = match fs::read_dir(&p) {
        Ok(rd) => rd,
        Err(_) => return 0,
    };
    let list = echo_runtime_list_new();
    for ent in rd.flatten() {
        let name = ent.file_name();
        let s = name.to_string_lossy().into_owned();
        let h = string_to_handle(s);
        unsafe {
            echo_runtime_list_push(list, h);
        }
    }
    list
}

/// Remove an empty directory. **0** ok, **-1** fail.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_remove_dir(path: i64) -> i64 {
    let Some(p) = path_from_handle(path) else {
        return -1;
    };
    match fs::remove_dir(&p) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Copy file from → to. **0** ok, **-1** fail.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_copy(from: i64, to: i64) -> i64 {
    let Some(src) = path_from_handle(from) else {
        return -1;
    };
    let Some(dst) = path_from_handle(to) else {
        return -1;
    };
    match fs::copy(&src, &dst) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

/// Rename / move path. **0** ok, **-1** fail.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_rename(from: i64, to: i64) -> i64 {
    let Some(src) = path_from_handle(from) else {
        return -1;
    };
    let Some(dst) = path_from_handle(to) else {
        return -1;
    };
    match fs::rename(&src, &dst) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

fn system_time_ms(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => {
            let ms = d.as_millis();
            if ms > i64::MAX as u128 {
                i64::MAX
            } else {
                ms as i64
            }
        }
        Err(_) => 0,
    }
}

/// Metadata as a list of five ints:
/// `[len, is_file, is_dir, is_symlink, modified_ms]`.
/// **0** on failure (so callers can branch without comparing a struct handle to 0).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_metadata(path: i64) -> i64 {
    let Some(p) = path_from_handle(path) else {
        return 0;
    };
    let meta = match fs::symlink_metadata(&p) {
        Ok(m) => m,
        Err(_) => return 0,
    };
    let ft = meta.file_type();
    let list = echo_runtime_list_new();
    let vals = [
        meta.len() as i64,
        if ft.is_file() { 1 } else { 0 },
        if ft.is_dir() { 1 } else { 0 },
        if ft.is_symlink() { 1 } else { 0 },
        meta.modified().map(system_time_ms).unwrap_or(0),
    ];
    for v in vals {
        unsafe {
            echo_runtime_list_push(list, v);
        }
    }
    list
}

/// Open for reading. File handle, or **0** on failure.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_open_read(path: i64) -> i64 {
    let Some(p) = path_from_handle(path) else {
        return 0;
    };
    match File::open(&p) {
        Ok(f) => file_to_handle(f),
        Err(_) => 0,
    }
}

/// Create/truncate for writing. File handle, or **0** on failure.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_open_write(path: i64) -> i64 {
    let Some(p) = path_from_handle(path) else {
        return 0;
    };
    match OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&p)
    {
        Ok(f) => file_to_handle(f),
        Err(_) => 0,
    }
}

/// Open for append (create if missing). File handle, or **0** on failure.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_open_append(path: i64) -> i64 {
    let Some(p) = path_from_handle(path) else {
        return 0;
    };
    match OpenOptions::new().append(true).create(true).open(&p) {
        Ok(f) => file_to_handle(f),
        Err(_) => 0,
    }
}

/// Read up to `limit` bytes from an open file.
///
/// - **0** — error / bad handle / limit ≤ 0
/// - empty bytes handle — EOF
/// - non-empty bytes — data
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_file_read(handle: i64, limit: i64) -> i64 {
    if limit <= 0 {
        return 0;
    }
    let Some(ef) = file_mut(handle) else {
        return 0;
    };
    let Some(f) = ef.inner.as_mut() else {
        return 0;
    };
    let n = limit.min(i64::from(u32::MAX)) as usize;
    let mut buf = vec![0u8; n];
    match f.read(&mut buf) {
        Ok(0) => bytes_to_handle(Vec::new()),
        Ok(got) => {
            buf.truncate(got);
            bytes_to_handle(buf)
        }
        Err(_) => 0,
    }
}

/// Write bytes/string to an open file. **0** ok, **-1** fail.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_file_write(handle: i64, data: i64) -> i64 {
    let Some(ef) = file_mut(handle) else {
        return -1;
    };
    let Some(f) = ef.inner.as_mut() else {
        return -1;
    };
    let Some(bytes) = payload_bytes(data) else {
        return -1;
    };
    match f.write_all(&bytes) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Seek to absolute byte offset. Returns new position, or **-1** on fail.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_file_seek(handle: i64, pos: i64) -> i64 {
    if pos < 0 {
        return -1;
    }
    let Some(ef) = file_mut(handle) else {
        return -1;
    };
    let Some(f) = ef.inner.as_mut() else {
        return -1;
    };
    match f.seek(SeekFrom::Start(pos as u64)) {
        Ok(p) => p as i64,
        Err(_) => -1,
    }
}

/// Drop a streaming file handle (closes the OS file).
pub(crate) fn free_file_object(handle: i64) {
    let _ = unsafe { Box::from_raw(handle as *mut EchoFile) };
}

/// Close an open file handle (idempotent).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_fs_file_close(handle: i64) {
    let Some(ef) = file_mut(handle) else {
        return;
    };
    ef.inner.take();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::echo_runtime_string_from_utf8;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn s(text: &str) -> i64 {
        unsafe { echo_runtime_string_from_utf8(text.as_ptr(), text.len()) }
    }

    fn tmp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("echo_fs_test_{nanos}"))
    }

    #[test]
    fn file_write_read_remove() {
        let root = tmp_root();
        fs::create_dir_all(&root).unwrap();
        let file = root.join("a.txt");
        let path = s(file.to_str().unwrap());

        assert_eq!(echo_runtime_fs_exists(path), 0);
        assert_eq!(echo_runtime_fs_write(path, s("hi")), 0);
        assert_eq!(echo_runtime_fs_exists(path), 1);
        assert_eq!(echo_runtime_fs_is_file(path), 1);
        assert_eq!(echo_runtime_fs_is_dir(path), 0);

        let data = echo_runtime_fs_read(path);
        assert_ne!(data, 0);
        assert_eq!(bytes_data(data).as_deref(), Some(b"hi".as_slice()));

        assert_eq!(echo_runtime_fs_remove(path), 0);
        assert_eq!(echo_runtime_fs_exists(path), 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn dir_create_list_remove() {
        let root = tmp_root();
        let root_s = s(root.to_str().unwrap());
        assert_eq!(echo_runtime_fs_create_dir_all(root_s), 0);
        assert_eq!(echo_runtime_fs_is_dir(root_s), 1);

        let child = root.join("child");
        let child_s = s(child.to_str().unwrap());
        assert_eq!(echo_runtime_fs_create_dir(child_s), 0);

        let names = echo_runtime_fs_read_dir(root_s);
        assert_ne!(names, 0);
        let n = unsafe { crate::echo_runtime_list_len(names) };
        assert!(n >= 1);
        let mut found = false;
        for i in 0..n {
            let h = unsafe { crate::echo_runtime_list_get(names, i) };
            if string_data(h).as_deref() == Some("child") {
                found = true;
            }
        }
        assert!(found, "read_dir should list the child entry");

        assert_eq!(echo_runtime_fs_remove_dir(child_s), 0);
        assert_eq!(echo_runtime_fs_remove_dir(root_s), 0);
    }

    #[test]
    fn join_and_missing() {
        let joined = echo_runtime_fs_join(s("/tmp"), s("x"));
        let text = string_data(joined).unwrap();
        assert!(text.contains("x"), "{text}");
        assert_eq!(echo_runtime_fs_read(s("/__echo_fs_missing_xyz__")), 0);
        assert_eq!(echo_runtime_fs_write(0, s("x")), -1);
    }

    #[test]
    fn copy_rename_metadata() {
        let root = tmp_root();
        fs::create_dir_all(&root).unwrap();
        let a = root.join("a.txt");
        let b = root.join("b.txt");
        let c = root.join("c.txt");
        let pa = s(a.to_str().unwrap());
        let pb = s(b.to_str().unwrap());
        let pc = s(c.to_str().unwrap());
        assert_eq!(echo_runtime_fs_write(pa, s("data")), 0);
        assert_eq!(echo_runtime_fs_copy(pa, pb), 0);
        assert_eq!(bytes_data(echo_runtime_fs_read(pb)).as_deref(), Some(b"data".as_slice()));
        assert_eq!(echo_runtime_fs_rename(pb, pc), 0);
        assert_eq!(echo_runtime_fs_exists(pb), 0);
        assert_eq!(echo_runtime_fs_exists(pc), 1);

        let meta = echo_runtime_fs_metadata(pa);
        assert_ne!(meta, 0);
        let len = unsafe { crate::echo_runtime_list_get(meta, 0) };
        assert_eq!(len, 4);
        let is_file = unsafe { crate::echo_runtime_list_get(meta, 1) };
        assert_eq!(is_file, 1);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn streaming_read_write() {
        let root = tmp_root();
        fs::create_dir_all(&root).unwrap();
        let path = root.join("stream.bin");
        let p = s(path.to_str().unwrap());

        let w = echo_runtime_fs_open_write(p);
        assert_ne!(w, 0);
        assert_eq!(echo_runtime_fs_file_write(w, s("abcd")), 0);
        echo_runtime_fs_file_close(w);

        let r = echo_runtime_fs_open_read(p);
        assert_ne!(r, 0);
        let chunk = echo_runtime_fs_file_read(r, 2);
        assert_eq!(bytes_data(chunk).as_deref(), Some(b"ab".as_slice()));
        let chunk2 = echo_runtime_fs_file_read(r, 10);
        assert_eq!(bytes_data(chunk2).as_deref(), Some(b"cd".as_slice()));
        let eof = echo_runtime_fs_file_read(r, 10);
        assert_eq!(bytes_data(eof).as_deref(), Some(b"".as_slice()));
        echo_runtime_fs_file_close(r);

        let sk = echo_runtime_fs_open_read(p);
        assert_ne!(sk, 0);
        assert_eq!(echo_runtime_fs_file_seek(sk, 2), 2);
        let mid = echo_runtime_fs_file_read(sk, 10);
        assert_eq!(bytes_data(mid).as_deref(), Some(b"cd".as_slice()));
        assert_eq!(echo_runtime_fs_file_seek(sk, -1), -1);
        echo_runtime_fs_file_close(sk);

        let a = echo_runtime_fs_open_append(p);
        assert_ne!(a, 0);
        assert_eq!(echo_runtime_fs_file_write(a, s("!")), 0);
        echo_runtime_fs_file_close(a);
        assert_eq!(
            bytes_data(echo_runtime_fs_read(p)).as_deref(),
            Some(b"abcd!".as_slice())
        );

        let _ = fs::remove_dir_all(&root);
    }

}
