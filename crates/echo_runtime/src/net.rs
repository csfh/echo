//! TCP/UDP sockets for `std/net` — nonblocking + park on the **mio** loop.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};

use crate::sched::IoRaw;
use crate::{
    bytes_data, bytes_to_handle, echo_runtime_struct_new, header_at, string_data, string_to_handle,
    struct_set_str, HEAP_MAGIC,
};

/// Portable raw handle for parking (Unix fd / Windows socket).
trait AsIoRaw {
    fn as_io_raw(&self) -> IoRaw;
}

#[cfg(unix)]
impl<T: std::os::fd::AsRawFd> AsIoRaw for T {
    fn as_io_raw(&self) -> IoRaw {
        self.as_raw_fd()
    }
}

#[cfg(windows)]
impl<T: std::os::windows::io::AsRawSocket> AsIoRaw for T {
    fn as_io_raw(&self) -> IoRaw {
        self.as_raw_socket()
    }
}

pub(crate) const KIND_TCP_LISTENER: u32 = 10;
pub(crate) const KIND_TCP_STREAM: u32 = 11;
pub(crate) const KIND_UDP_SOCKET: u32 = 12;

#[repr(C)]
struct EchoTcpListener {
    header: crate::HeapHeader,
    inner: Option<TcpListener>,
}

#[repr(C)]
struct EchoTcpStream {
    header: crate::HeapHeader,
    inner: Option<TcpStream>,
}

#[repr(C)]
struct EchoUdpSocket {
    header: crate::HeapHeader,
    inner: Option<UdpSocket>,
}

fn tcp_listener_header() -> crate::HeapHeader {
    crate::HeapHeader {
        magic: HEAP_MAGIC,
        kind: KIND_TCP_LISTENER,
        promotion_epoch: 0,
    }
}

fn tcp_stream_header() -> crate::HeapHeader {
    crate::HeapHeader {
        magic: HEAP_MAGIC,
        kind: KIND_TCP_STREAM,
        promotion_epoch: 0,
    }
}

fn udp_header() -> crate::HeapHeader {
    crate::HeapHeader {
        magic: HEAP_MAGIC,
        kind: KIND_UDP_SOCKET,
        promotion_epoch: 0,
    }
}

fn parse_addr(s: &str) -> Option<SocketAddr> {
    if let Ok(a) = s.parse::<SocketAddr>() {
        return Some(a);
    }
    s.to_socket_addrs().ok()?.next()
}

fn addr_string(a: SocketAddr) -> String {
    a.to_string()
}

fn would_block(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::Interrupted
}

/// After WouldBlock: arm → one more try → wait if still blocked (edge-safe).
enum AfterWb {
    Done,
    Waited,
}

fn after_would_block_readable(fd: &impl AsIoRaw, mut retry_ok: impl FnMut() -> bool) -> AfterWb {
    let raw = fd.as_io_raw();
    let tok = crate::sched::arm_fd(raw, true, false);
    if retry_ok() {
        crate::sched::disarm_fd(tok, raw);
        return AfterWb::Done;
    }
    crate::sched::wait_fd(tok, raw);
    AfterWb::Waited
}

fn after_would_block_writable(fd: &impl AsIoRaw, mut retry_ok: impl FnMut() -> bool) -> AfterWb {
    let raw = fd.as_io_raw();
    let tok = crate::sched::arm_fd(raw, false, true);
    if retry_ok() {
        crate::sched::disarm_fd(tok, raw);
        return AfterWb::Done;
    }
    crate::sched::wait_fd(tok, raw);
    AfterWb::Waited
}

/// `tcp_listen(addr_string) -> listener handle` (0 on failure).
///
/// # Safety
/// `addr` is 0 or a string handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_tcp_listen(addr: i64) -> i64 {
    let Some(s) = string_data(addr) else {
        return 0;
    };
    let Some(sa) = parse_addr(&s) else {
        return 0;
    };
    let Ok(listener) = TcpListener::bind(sa) else {
        return 0;
    };
    let _ = listener.set_nonblocking(true);
    let h = Box::new(EchoTcpListener {
        header: tcp_listener_header(),
        inner: Some(listener),
    });
    crate::heap_to_handle(h)
}

/// `tcp_accept(listener) -> struct { conn, remote }` (empty on failure).
///
/// Parks the current task worker on WouldBlock (other workers keep running).
///
/// # Safety
/// `listener` is 0 or a tcp listener handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_tcp_accept(listener: i64) -> i64 {
    let out = echo_runtime_struct_new();
    if listener == 0 {
        return out;
    }
    let Some(h) = (unsafe { header_at(listener) }) else {
        return out;
    };
    if unsafe { (*h).kind } != KIND_TCP_LISTENER {
        return out;
    }
    let lis = unsafe { &mut *(listener as *mut EchoTcpListener) };
    loop {
        let Some(ref l) = lis.inner else {
            return out;
        };
        match l.accept() {
            Ok((stream, peer)) => {
                let _ = stream.set_nonblocking(true);
                let conn = Box::new(EchoTcpStream {
                    header: tcp_stream_header(),
                    inner: Some(stream),
                });
                let conn_h = crate::heap_to_handle(conn);
                struct_set_str(out, "conn", conn_h);
                struct_set_str(out, "remote", string_to_handle(addr_string(peer)));
                return out;
            }
            Err(e) if would_block(&e) => {
                let mut filled = false;
                after_would_block_readable(l, || {
                    let Some(ref l2) = lis.inner else {
                        return true;
                    };
                    match l2.accept() {
                        Ok((stream, peer)) => {
                            let _ = stream.set_nonblocking(true);
                            let conn = Box::new(EchoTcpStream {
                                header: tcp_stream_header(),
                                inner: Some(stream),
                            });
                            let conn_h = crate::heap_to_handle(conn);
                            struct_set_str(out, "conn", conn_h);
                            struct_set_str(out, "remote", string_to_handle(addr_string(peer)));
                            filled = true;
                            true
                        }
                        Err(e2) if would_block(&e2) => false,
                        Err(_) => true,
                    }
                });
                if filled {
                    return out;
                }
            }
            Err(_) => return out,
        }
    }
}

/// `tcp_connect(addr_string) -> stream handle` (0 on failure).
///
/// # Safety
/// `addr` is 0 or a string handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_tcp_connect(addr: i64) -> i64 {
    let Some(s) = string_data(addr) else {
        return 0;
    };
    let Some(sa) = parse_addr(&s) else {
        return 0;
    };
    // Connect is typically blocking-fast on loopback; use nonblocking stream after.
    let Ok(stream) = TcpStream::connect(sa) else {
        return 0;
    };
    let _ = stream.set_nonblocking(true);
    let h = Box::new(EchoTcpStream {
        header: tcp_stream_header(),
        inner: Some(stream),
    });
    crate::heap_to_handle(h)
}

/// `tcp_read(stream, limit) -> bytes handle` (empty on failure / EOF).
///
/// # Safety
/// `stream` is 0 or a tcp stream handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_tcp_read(stream: i64, limit: i64) -> i64 {
    if stream == 0 || limit <= 0 {
        return bytes_to_handle(Vec::new());
    }
    let Some(h) = (unsafe { header_at(stream) }) else {
        return bytes_to_handle(Vec::new());
    };
    if unsafe { (*h).kind } != KIND_TCP_STREAM {
        return bytes_to_handle(Vec::new());
    }
    let st = unsafe { &mut *(stream as *mut EchoTcpStream) };
    let lim = (limit as usize).min(16 * 1024 * 1024);
    let mut buf = vec![0u8; lim];
    loop {
        let Some(ref mut s) = st.inner else {
            return bytes_to_handle(Vec::new());
        };
        match s.read(&mut buf) {
            Ok(0) => return bytes_to_handle(Vec::new()),
            Ok(n) => {
                buf.truncate(n);
                return bytes_to_handle(buf);
            }
            Err(e) if would_block(&e) => {
                let raw = s.as_io_raw();
                let tok = crate::sched::arm_fd(raw, true, false);
                // Retry after arm (edge race).
                let Some(ref mut s2) = st.inner else {
                    crate::sched::disarm_fd(tok, raw);
                    return bytes_to_handle(Vec::new());
                };
                match s2.read(&mut buf) {
                    Ok(0) => {
                        crate::sched::disarm_fd(tok, raw);
                        return bytes_to_handle(Vec::new());
                    }
                    Ok(n) => {
                        crate::sched::disarm_fd(tok, raw);
                        buf.truncate(n);
                        return bytes_to_handle(buf);
                    }
                    Err(e2) if would_block(&e2) => {
                        crate::sched::wait_fd(tok, raw);
                    }
                    Err(_) => {
                        crate::sched::disarm_fd(tok, raw);
                        return bytes_to_handle(Vec::new());
                    }
                }
            }
            Err(_) => return bytes_to_handle(Vec::new()),
        }
    }
}

/// `tcp_write(stream, data) -> bytes written` (−1 on failure).
///
/// # Safety
/// `stream` is 0 or a tcp stream handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_tcp_write(stream: i64, data: i64) -> i64 {
    if stream == 0 {
        return -1;
    }
    let bytes = bytes_data(data)
        .or_else(|| string_data(data).map(|s| s.into_bytes()))
        .unwrap_or_default();
    let Some(h) = (unsafe { header_at(stream) }) else {
        return -1;
    };
    if unsafe { (*h).kind } != KIND_TCP_STREAM {
        return -1;
    }
    let st = unsafe { &mut *(stream as *mut EchoTcpStream) };
    let mut written = 0usize;
    loop {
        let Some(ref mut s) = st.inner else {
            return -1;
        };
        match s.write(&bytes[written..]) {
            Ok(0) => return -1,
            Ok(n) => {
                written += n;
                if written >= bytes.len() {
                    let _ = s.flush();
                    return bytes.len() as i64;
                }
            }
            Err(e) if would_block(&e) => {
                let raw = s.as_io_raw();
                let tok = crate::sched::arm_fd(raw, false, true);
                let Some(ref mut s2) = st.inner else {
                    crate::sched::disarm_fd(tok, raw);
                    return -1;
                };
                match s2.write(&bytes[written..]) {
                    Ok(0) => {
                        crate::sched::disarm_fd(tok, raw);
                        return -1;
                    }
                    Ok(n) => {
                        written += n;
                        if written >= bytes.len() {
                            crate::sched::disarm_fd(tok, raw);
                            let _ = s2.flush();
                            return bytes.len() as i64;
                        }
                        crate::sched::disarm_fd(tok, raw);
                    }
                    Err(e2) if would_block(&e2) => {
                        crate::sched::wait_fd(tok, raw);
                    }
                    Err(_) => {
                        crate::sched::disarm_fd(tok, raw);
                        return -1;
                    }
                }
            }
            Err(_) => return -1,
        }
    }
}

/// Drop a TCP/UDP heap object after taking the OS resource (scope dispose).
pub(crate) fn free_net_object(handle: i64, kind: u32) {
    match kind {
        KIND_TCP_LISTENER => {
            let _ = unsafe { Box::from_raw(handle as *mut EchoTcpListener) };
        }
        KIND_TCP_STREAM => {
            let _ = unsafe { Box::from_raw(handle as *mut EchoTcpStream) };
        }
        KIND_UDP_SOCKET => {
            let _ = unsafe { Box::from_raw(handle as *mut EchoUdpSocket) };
        }
        _ => {}
    }
}

/// Close a TCP listener or stream handle.
///
/// # Safety
/// `handle` is 0 or a tcp listener/stream handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_tcp_close(handle: i64) {
    if handle == 0 {
        return;
    }
    let Some(h) = (unsafe { header_at(handle) }) else {
        return;
    };
    match unsafe { (*h).kind } {
        KIND_TCP_LISTENER => {
            let lis = unsafe { &mut *(handle as *mut EchoTcpListener) };
            lis.inner.take();
        }
        KIND_TCP_STREAM => {
            let st = unsafe { &mut *(handle as *mut EchoTcpStream) };
            st.inner.take();
        }
        _ => {}
    }
}

/// `udp_bind(addr_string) -> udp handle` (0 on failure).
///
/// # Safety
/// `addr` is 0 or a string handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_udp_bind(addr: i64) -> i64 {
    let Some(s) = string_data(addr) else {
        return 0;
    };
    let Some(sa) = parse_addr(&s) else {
        return 0;
    };
    let Ok(sock) = UdpSocket::bind(sa) else {
        return 0;
    };
    let _ = sock.set_nonblocking(true);
    let h = Box::new(EchoUdpSocket {
        header: udp_header(),
        inner: Some(sock),
    });
    crate::heap_to_handle(h)
}

/// `udp_send_to(sock, data, addr_string) -> bytes sent` (−1 on failure).
///
/// # Safety
/// Handles are 0 or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_udp_send_to(sock: i64, data: i64, addr: i64) -> i64 {
    if sock == 0 {
        return -1;
    }
    let bytes = bytes_data(data)
        .or_else(|| string_data(data).map(|s| s.into_bytes()))
        .unwrap_or_default();
    let Some(addr_s) = string_data(addr) else {
        return -1;
    };
    let Some(sa) = parse_addr(&addr_s) else {
        return -1;
    };
    let Some(h) = (unsafe { header_at(sock) }) else {
        return -1;
    };
    if unsafe { (*h).kind } != KIND_UDP_SOCKET {
        return -1;
    }
    let u = unsafe { &mut *(sock as *mut EchoUdpSocket) };
    loop {
        let Some(ref s) = u.inner else {
            return -1;
        };
        match s.send_to(&bytes, sa) {
            Ok(n) => return n as i64,
            Err(e) if would_block(&e) => {
                let mut result = None;
                after_would_block_writable(s, || {
                    let Some(ref s2) = u.inner else {
                        result = Some(-1);
                        return true;
                    };
                    match s2.send_to(&bytes, sa) {
                        Ok(n) => {
                            result = Some(n as i64);
                            true
                        }
                        Err(e2) if would_block(&e2) => false,
                        Err(_) => {
                            result = Some(-1);
                            true
                        }
                    }
                });
                if let Some(r) = result {
                    return r;
                }
            }
            Err(_) => return -1,
        }
    }
}

/// `udp_recv_from(sock, limit) -> struct { data, from }` (empty data on failure).
///
/// # Safety
/// `sock` is 0 or a udp handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_udp_recv_from(sock: i64, limit: i64) -> i64 {
    let out = echo_runtime_struct_new();
    struct_set_str(out, "data", bytes_to_handle(Vec::new()));
    struct_set_str(out, "from", string_to_handle(String::new()));
    if sock == 0 || limit <= 0 {
        return out;
    }
    let Some(h) = (unsafe { header_at(sock) }) else {
        return out;
    };
    if unsafe { (*h).kind } != KIND_UDP_SOCKET {
        return out;
    }
    let u = unsafe { &mut *(sock as *mut EchoUdpSocket) };
    let lim = (limit as usize).min(16 * 1024 * 1024);
    let mut buf = vec![0u8; lim];
    loop {
        let Some(ref s) = u.inner else {
            return out;
        };
        match s.recv_from(&mut buf) {
            Ok((n, peer)) => {
                buf.truncate(n);
                struct_set_str(out, "data", bytes_to_handle(buf));
                struct_set_str(out, "from", string_to_handle(addr_string(peer)));
                return out;
            }
            Err(e) if would_block(&e) => {
                let mut filled = false;
                after_would_block_readable(s, || {
                    let Some(ref s2) = u.inner else {
                        return true;
                    };
                    match s2.recv_from(&mut buf) {
                        Ok((n, peer)) => {
                            buf.truncate(n);
                            struct_set_str(out, "data", bytes_to_handle(buf.clone()));
                            struct_set_str(out, "from", string_to_handle(addr_string(peer)));
                            filled = true;
                            true
                        }
                        Err(e2) if would_block(&e2) => false,
                        Err(_) => true,
                    }
                });
                if filled {
                    return out;
                }
            }
            Err(_) => return out,
        }
    }
}

/// Close a UDP socket handle.
///
/// # Safety
/// `handle` is 0 or a udp handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn echo_runtime_udp_close(handle: i64) {
    if handle == 0 {
        return;
    }
    let Some(h) = (unsafe { header_at(handle) }) else {
        return;
    };
    if unsafe { (*h).kind } != KIND_UDP_SOCKET {
        return;
    }
    let u = unsafe { &mut *(handle as *mut EchoUdpSocket) };
    u.inner.take();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::echo_runtime_string_from_utf8;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn tcp_loopback_echo_nonblocking() {
        let addr = "127.0.0.1:39886";
        let a = unsafe { echo_runtime_string_from_utf8(addr.as_ptr(), addr.len()) };
        let lis = unsafe { echo_runtime_tcp_listen(a) };
        if lis == 0 {
            return;
        }

        // Accept parks until client connects — run accept on another thread.
        let barrier = Arc::new(Barrier::new(2));
        let b2 = barrier.clone();
        let accept_thr = thread::spawn(move || {
            b2.wait();
            unsafe { echo_runtime_tcp_accept(lis) }
        });
        barrier.wait();
        thread::sleep(Duration::from_millis(20));
        let client = unsafe { echo_runtime_tcp_connect(a) };
        assert_ne!(client, 0, "connect");
        let acc = accept_thr.join().expect("accept thr");
        let conn_k = b"conn";
        let server = unsafe { crate::echo_runtime_struct_get(acc, conn_k.as_ptr(), conn_k.len()) };
        assert_ne!(server, 0, "accept conn");

        let msg = b"ping";
        let data = unsafe { echo_runtime_string_from_utf8(msg.as_ptr(), msg.len()) };
        let n = unsafe { echo_runtime_tcp_write(client, data) };
        assert_eq!(n, 4);

        let got = unsafe { echo_runtime_tcp_read(server, 64) };
        let bytes = bytes_data(got).unwrap_or_default();
        assert_eq!(bytes, b"ping");

        unsafe {
            echo_runtime_tcp_close(client);
            echo_runtime_tcp_close(server);
            echo_runtime_tcp_close(lis);
        }
    }

    #[test]
    fn udp_loopback_nonblocking() {
        let addr = "127.0.0.1:39887";
        let a = unsafe { echo_runtime_string_from_utf8(addr.as_ptr(), addr.len()) };
        let sock = unsafe { echo_runtime_udp_bind(a) };
        if sock == 0 {
            return;
        }
        let msg = b"hi";
        let data = unsafe { echo_runtime_string_from_utf8(msg.as_ptr(), msg.len()) };
        let n = unsafe { echo_runtime_udp_send_to(sock, data, a) };
        assert_eq!(n, 2);
        let pkt = unsafe { echo_runtime_udp_recv_from(sock, 64) };
        let data_k = b"data";
        unsafe {
            let d = crate::echo_runtime_struct_get(pkt, data_k.as_ptr(), data_k.len());
            let bytes = bytes_data(d).unwrap_or_default();
            assert_eq!(bytes, b"hi");
            echo_runtime_udp_close(sock);
        }
    }
}
