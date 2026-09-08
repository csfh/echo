//! Blocking TLS streams (rustls) for `std/net/tls` — local-cert friendly.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::Once;

use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{
    ClientConfig, ClientConnection, RootCertStore, ServerConfig, ServerConnection, StreamOwned,
};

static CRYPTO: Once = Once::new();

fn ensure_crypto() {
    CRYPTO.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

use crate::{HEAP_MAGIC, bytes_data, bytes_to_handle, header_at, string_data};

const KIND_TLS_LISTENER: u32 = 20;
const KIND_TLS_STREAM: u32 = 21;

enum TlsInner {
    Client(StreamOwned<ClientConnection, TcpStream>),
    Server(StreamOwned<ServerConnection, TcpStream>),
}

#[repr(C)]
pub(crate) struct EchoTlsListener {
    header: crate::HeapHeader,
    inner: Option<TcpListener>,
}

#[repr(C)]
pub(crate) struct EchoTlsStream {
    header: crate::HeapHeader,
    inner: Option<TlsInner>,
}

fn tls_listener_header() -> crate::HeapHeader {
    crate::HeapHeader {
        magic: HEAP_MAGIC,
        kind: KIND_TLS_LISTENER,
        promotion_epoch: 0,
    }
}

fn tls_stream_header() -> crate::HeapHeader {
    crate::HeapHeader {
        magic: HEAP_MAGIC,
        kind: KIND_TLS_STREAM,
        promotion_epoch: 0,
    }
}

fn parse_certs(pem: &str) -> Option<Vec<CertificateDer<'static>>> {
    let mut reader = std::io::Cursor::new(pem.as_bytes());
    let certs: Result<Vec<_>, _> = rustls_pemfile::certs(&mut reader).collect();
    let certs = certs.ok()?;
    if certs.is_empty() {
        return None;
    }
    Some(certs)
}

fn parse_key(pem: &str) -> Option<PrivateKeyDer<'static>> {
    let mut reader = std::io::Cursor::new(pem.as_bytes());
    for item in rustls_pemfile::read_all(&mut reader) {
        match item.ok()? {
            rustls_pemfile::Item::Pkcs8Key(k) => return Some(PrivateKeyDer::Pkcs8(k)),
            rustls_pemfile::Item::Pkcs1Key(k) => return Some(PrivateKeyDer::Pkcs1(k)),
            rustls_pemfile::Item::Sec1Key(k) => return Some(PrivateKeyDer::Sec1(k)),
            _ => {}
        }
    }
    None
}

fn client_config_with_ca(ca_pem: &str) -> Option<ClientConfig> {
    let mut roots = RootCertStore::empty();
    let certs = parse_certs(ca_pem)?;
    for c in certs {
        roots.add(c).ok()?;
    }
    let cfg = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Some(cfg)
}

/// Platform/OS trust store, then Mozilla roots (scratch images have no OS certs).
fn client_config_platform_roots() -> Option<ClientConfig> {
    let mut roots = RootCertStore::empty();
    let certs = rustls_native_certs::load_native_certs();
    for c in certs.certs {
        let _ = roots.add(c);
    }
    if roots.is_empty() {
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    }
    if roots.is_empty() {
        return None;
    }
    let cfg = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Some(cfg)
}

fn server_config(cert_pem: &str, key_pem: &str) -> Option<ServerConfig> {
    let certs = parse_certs(cert_pem)?;
    let key = parse_key(key_pem)?;
    ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .ok()
}

/// `tls_listen(addr) -> listener handle` (0 on failure). Blocking accept sockets.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_tls_listen(addr: i64) -> i64 {
    ensure_crypto();
    let Some(s) = string_data(addr) else {
        return 0;
    };
    let Ok(lis) = (|| -> std::io::Result<TcpListener> {
        let lis = TcpListener::bind(s)?;
        lis.set_nonblocking(false)?;
        Ok(lis)
    })() else {
        return 0;
    };
    let boxed = Box::new(EchoTlsListener {
        header: tls_listener_header(),
        inner: Some(lis),
    });
    // Like TCP sockets: do **not** note_heap_alloc — scope free would reclaim
    // the handle when the std wrapper returns a product holding it.
    Box::into_raw(boxed) as i64
}

/// `tls_accept(listener, cert_pem, key_pem) -> stream handle` (0 on failure).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_tls_accept(listener: i64, cert_pem: i64, key_pem: i64) -> i64 {
    ensure_crypto();
    if listener == 0 {
        eprintln!("echo: tls_accept: null listener");
        return 0;
    }
    let Some(cert) = string_data(cert_pem) else {
        return 0;
    };
    let Some(key) = string_data(key_pem) else {
        return 0;
    };
    let Some(cfg) = server_config(&cert, &key) else {
        eprintln!("echo: tls_accept: server_config failed");
        return 0;
    };
    let cfg = Arc::new(cfg);
    unsafe {
        let Some(h) = header_at(listener) else {
            eprintln!("echo: tls_accept: bad header");
            return 0;
        };
        if (*h).kind != KIND_TLS_LISTENER {
            eprintln!("echo: tls_accept: bad kind {}", (*h).kind);
            return 0;
        }
        let lis = &mut *(listener as *mut EchoTlsListener);
        let Some(inner) = lis.inner.as_ref() else {
            eprintln!("echo: tls_accept: no inner");
            return 0;
        };
        let Ok((sock, _)) = inner.accept() else {
            eprintln!("echo: tls_accept: accept failed");
            return 0;
        };
        let _ = sock.set_read_timeout(Some(std::time::Duration::from_secs(5)));
        let _ = sock.set_write_timeout(Some(std::time::Duration::from_secs(5)));
        let Ok(conn) = ServerConnection::new(cfg) else {
            eprintln!("echo: tls_accept: ServerConnection::new failed");
            return 0;
        };
        let stream = StreamOwned::new(conn, sock);
        let boxed = Box::new(EchoTlsStream {
            header: tls_stream_header(),
            inner: Some(TlsInner::Server(stream)),
        });
        Box::into_raw(boxed) as i64
    }
}

/// `tls_connect(host, port, server_name, ca_pem) -> stream` (0 on failure).
/// Non-empty `ca_pem` is a local CA/bundle; empty string uses platform roots.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_tls_connect(
    host: i64,
    port: i64,
    server_name: i64,
    ca_pem: i64,
) -> i64 {
    ensure_crypto();
    let Some(host_s) = string_data(host) else {
        return 0;
    };
    let Some(sni) = string_data(server_name) else {
        return 0;
    };
    let Some(ca) = string_data(ca_pem) else {
        return 0;
    };
    let Some(cfg) = (if ca.is_empty() {
        client_config_platform_roots()
    } else {
        client_config_with_ca(&ca)
    }) else {
        return 0;
    };
    let cfg = Arc::new(cfg);
    let addr = format!("{host_s}:{port}");
    let Ok(mut addrs) = addr.to_socket_addrs() else {
        return 0;
    };
    let Some(sock_addr) = addrs.next() else {
        return 0;
    };
    let Ok(sock) = TcpStream::connect(sock_addr) else {
        return 0;
    };
    let _ = sock.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let _ = sock.set_write_timeout(Some(std::time::Duration::from_secs(5)));
    let Ok(name) = ServerName::try_from(sni.to_string()) else {
        return 0;
    };
    let Ok(conn) = ClientConnection::new(cfg, name) else {
        return 0;
    };
    let stream = StreamOwned::new(conn, sock);
    let boxed = Box::new(EchoTlsStream {
        header: tls_stream_header(),
        inner: Some(TlsInner::Client(stream)),
    });
    Box::into_raw(boxed) as i64
}

/// `tls_read(stream, limit) -> bytes` (0 on failure / empty).
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_tls_read(stream: i64, limit: i64) -> i64 {
    if stream == 0 {
        return 0;
    }
    let limit = if limit <= 0 { 0 } else { limit as usize };
    if limit == 0 {
        return bytes_to_handle(Vec::new());
    }
    unsafe {
        let Some(h) = header_at(stream) else {
            return 0;
        };
        if (*h).kind != KIND_TLS_STREAM {
            return 0;
        }
        let st = &mut *(stream as *mut EchoTlsStream);
        let Some(inner) = st.inner.as_mut() else {
            return 0;
        };
        let mut buf = vec![0u8; limit];
        let n = match inner {
            TlsInner::Client(s) => s.read(&mut buf),
            TlsInner::Server(s) => s.read(&mut buf),
        };
        match n {
            Ok(0) => bytes_to_handle(Vec::new()),
            Ok(n) => {
                buf.truncate(n);
                bytes_to_handle(buf)
            }
            Err(_) => 0,
        }
    }
}

/// `tls_write(stream, data) -> 0 ok, -1 fail`. data is string or bytes.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_tls_write(stream: i64, data: i64) -> i64 {
    if stream == 0 {
        return -1;
    }
    let bytes = if let Some(s) = string_data(data) {
        s.into_bytes()
    } else if let Some(b) = bytes_data(data) {
        b.to_vec()
    } else {
        return -1;
    };
    unsafe {
        let Some(h) = header_at(stream) else {
            return 0;
        };
        if (*h).kind != KIND_TLS_STREAM {
            return -1;
        }
        let st = &mut *(stream as *mut EchoTlsStream);
        let Some(inner) = st.inner.as_mut() else {
            return -1;
        };
        let r = match inner {
            TlsInner::Client(s) => s.write_all(&bytes).and_then(|_| s.flush()),
            TlsInner::Server(s) => s.write_all(&bytes).and_then(|_| s.flush()),
        };
        match r {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("echo: tls_write: {e}");
                -1
            }
        }
    }
}

/// `tls_close(stream)`.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_tls_close(stream: i64) {
    if stream == 0 {
        return;
    }
    unsafe {
        let Some(h) = header_at(stream) else {
            return;
        };
        if (*h).kind != KIND_TLS_STREAM {
            return;
        }
        let st = &mut *(stream as *mut EchoTlsStream);
        st.inner = None;
    }
}

/// `tls_close_listener(listener)`.
#[unsafe(no_mangle)]
pub extern "C" fn echo_runtime_tls_close_listener(listener: i64) {
    if listener == 0 {
        return;
    }
    unsafe {
        let Some(h) = header_at(listener) else {
            return;
        };
        if (*h).kind != KIND_TLS_LISTENER {
            return;
        }
        let lis = &mut *(listener as *mut EchoTlsListener);
        lis.inner = None;
    }
}

/// Free TLS heap objects (called from scope free path if kind matches).
pub(crate) unsafe fn free_tls_object(ptr: *mut u8, kind: u32) {
    if kind == KIND_TLS_LISTENER {
        let _ = unsafe { Box::from_raw(ptr as *mut EchoTlsListener) };
    } else if kind == KIND_TLS_STREAM {
        let _ = unsafe { Box::from_raw(ptr as *mut EchoTlsStream) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::echo_runtime_string_from_utf8;
    use std::thread;
    use std::time::Duration;

    fn s(t: &str) -> i64 {
        unsafe { echo_runtime_string_from_utf8(t.as_ptr(), t.len()) }
    }

    fn load_fixture(name: &str) -> String {
        // tests run from crate dir or workspace root
        let paths = [
            format!("../../echo26/run/tls/certs/{name}"),
            format!("echo26/run/tls/certs/{name}"),
            format!("../echo26/run/tls/certs/{name}"),
        ];
        for p in paths {
            if let Ok(t) = std::fs::read_to_string(&p) {
                return t;
            }
        }
        panic!("missing TLS fixture {name}");
    }

    #[test]
    fn rustls_direct_loopback() {
        ensure_crypto();
        let cert_pem = load_fixture("cert.pem");
        let key_pem = load_fixture("key.pem");
        let ca_pem = load_fixture("ca.pem");
        let cfg_s = Arc::new(server_config(&cert_pem, &key_pem).expect("server cfg"));
        let cfg_c = Arc::new(client_config_with_ca(&ca_pem).expect("client cfg"));

        let lis = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = lis.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (sock, _) = lis.accept().unwrap();
            let conn = ServerConnection::new(cfg_s).unwrap();
            let mut st = StreamOwned::new(conn, sock);
            let mut buf = [0u8; 16];
            let n = st.read(&mut buf).unwrap();
            assert_eq!(&buf[..n], b"ping");
            st.write_all(b"pong").unwrap();
            st.flush().unwrap();
        });
        thread::sleep(Duration::from_millis(50));
        let sock = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let name = ServerName::try_from("localhost".to_string()).unwrap();
        let conn = ClientConnection::new(cfg_c, name).unwrap();
        let mut st = StreamOwned::new(conn, sock);
        st.write_all(b"ping").unwrap();
        st.flush().unwrap();
        let mut buf = [0u8; 16];
        let n = st.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"pong");
        server.join().unwrap();
    }

    #[test]
    fn tls_loopback_echo() {
        let cert = load_fixture("cert.pem");
        let key = load_fixture("key.pem");
        let ca = load_fixture("ca.pem");

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port() as i64;
        drop(listener);
        let addr = format!("127.0.0.1:{port}");
        let lis = echo_runtime_tls_listen(s(&addr));
        assert_ne!(lis, 0, "listen");

        let cert_s = s(&cert);
        let key_s = s(&key);
        let ca_s = s(&ca);
        let (tx, rx) = std::sync::mpsc::channel::<i64>();

        let server = thread::spawn(move || {
            let st = echo_runtime_tls_accept(lis, cert_s, key_s);
            let _ = tx.send(st);
            if st == 0 {
                return;
            }
            let got = echo_runtime_tls_read(st, 64);
            assert_ne!(got, 0);
            let msg = bytes_data(got).expect("server bytes");
            assert_eq!(msg, b"ping");
            assert_eq!(echo_runtime_tls_write(st, s("pong")), 0);
            echo_runtime_tls_close(st);
        });

        thread::sleep(Duration::from_millis(100));
        let cli = echo_runtime_tls_connect(s("127.0.0.1"), port, s("localhost"), ca_s);
        assert_ne!(cli, 0, "connect");
        assert_eq!(echo_runtime_tls_write(cli, s("ping")), 0);
        let got = echo_runtime_tls_read(cli, 64);
        let msg = bytes_data(got).expect("client bytes");
        assert_eq!(msg, b"pong");
        echo_runtime_tls_close(cli);
        let st = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("accept result");
        assert_ne!(st, 0, "accept");
        server.join().unwrap();
        echo_runtime_tls_close_listener(lis);
    }
}
