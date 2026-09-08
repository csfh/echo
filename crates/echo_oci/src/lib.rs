//! Daemonless scratch OCI image layout + registry push.
//!
//! Packaging only. Not a compiler backend (ADR 0002).

#![forbid(unsafe_code)]

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};

/// Stable crate identity.
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

/// Write a scratch linux image (`Entrypoint: ["/app"]`). Platform arch follows
/// the ELF machine.
///
/// `binary` is copied to `/app` (mode 0755). The layer is gzipped only when
/// that shrinks the tar. Returns the layout directory (`dest`).
pub fn write_scratch_image(binary: &Path, dest: &Path) -> Result<PathBuf, String> {
    let elf = fs::read(binary).map_err(|e| format!("read {}: {e}", binary.display()))?;
    if elf.len() < 4 || elf[0..4] != *b"\x7fELF" {
        return Err(format!("{} is not an ELF binary", binary.display()));
    }
    let arch = elf_oci_arch(&elf)?;
    let tar = ustar_regular_file("/app", 0o755, &elf)?;
    let tar_digest = sha256_hex(&tar);
    let gz = gzip(&tar);
    let (layer, layer_media) = if gz.len() < tar.len() {
        (gz, "application/vnd.oci.image.layer.v1.tar+gzip")
    } else {
        (tar, "application/vnd.oci.image.layer.v1.tar")
    };
    let layer_digest = sha256_hex(&layer);

    let config = format!(
        r#"{{"architecture":"{arch}","os":"linux","config":{{"Entrypoint":["/app"]}},"rootfs":{{"type":"layers","diff_ids":["sha256:{tar_digest}"]}}}}"#
    );
    let config_bytes = config.into_bytes();
    let config_digest = sha256_hex(&config_bytes);

    let manifest = format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json","config":{{"mediaType":"application/vnd.oci.image.config.v1+json","digest":"sha256:{config_digest}","size":{}}},"layers":[{{"mediaType":"{layer_media}","digest":"sha256:{layer_digest}","size":{}}}]}}"#,
        config_bytes.len(),
        layer.len()
    );
    let manifest_bytes = manifest.into_bytes();
    let manifest_digest = sha256_hex(&manifest_bytes);

    let index = format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.oci.image.index.v1+json","manifests":[{{"mediaType":"application/vnd.oci.image.manifest.v1+json","digest":"sha256:{manifest_digest}","size":{},"platform":{{"architecture":"{arch}","os":"linux"}}}}]}}"#,
        manifest_bytes.len()
    );

    let blobs = dest.join("blobs").join("sha256");
    fs::create_dir_all(&blobs).map_err(|e| format!("create {}: {e}", blobs.display()))?;
    write_blob(&blobs, &layer_digest, &layer)?;
    write_blob(&blobs, &config_digest, &config_bytes)?;
    write_blob(&blobs, &manifest_digest, &manifest_bytes)?;
    fs::write(dest.join("oci-layout"), r#"{"imageLayoutVersion":"1.0.0"}"#)
        .map_err(|e| format!("oci-layout: {e}"))?;
    fs::write(dest.join("index.json"), index).map_err(|e| format!("index.json: {e}"))?;
    Ok(dest.to_path_buf())
}

/// Push an OCI layout directory to `registry/name:tag` (distribution spec).
///
/// Auth: `XO_REGISTRY_USER` / `XO_REGISTRY_PASSWORD`, else anonymous.
pub fn push_layout(layout: &Path, reference: &str) -> Result<String, String> {
    let dest = parse_reference(reference)?;
    let index_txt =
        fs::read_to_string(layout.join("index.json")).map_err(|e| format!("index.json: {e}"))?;
    let manifest_digest = json_string_field(&index_txt, "digest")
        .ok_or_else(|| "index.json missing manifest digest".to_string())?;
    let hex = digest_hex(&manifest_digest)?;
    let manifest = read_blob(layout, hex)?;
    let config_digest = json_string_field(
        std::str::from_utf8(&manifest).map_err(|_| "manifest is not utf-8")?,
        "digest",
    )
    .ok_or_else(|| "manifest missing config digest".to_string())?;
    let layer_digest =
        last_json_string_field(std::str::from_utf8(&manifest).unwrap_or(""), "digest")
            .ok_or_else(|| "manifest missing layer digest".to_string())?;

    let auth = registry_auth();
    put_blob(&dest, layout, &config_digest, &auth)?;
    put_blob(&dest, layout, &layer_digest, &auth)?;
    put_manifest(&dest, &manifest, &auth)?;
    Ok(format!("{}/{}:{}", dest.host, dest.repository, dest.tag))
}

/// Parsed `host/repo:tag` (optional `https://` prefix).
#[derive(Debug, Clone)]
pub struct ImageRef {
    pub host: String,
    pub repository: String,
    pub tag: String,
    pub tls: bool,
}

fn parse_reference(raw: &str) -> Result<ImageRef, String> {
    let s = raw
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let tls = !raw.contains("http://");
    let (path, tag) = s
        .rsplit_once(':')
        .filter(|(left, tag)| !tag.contains('/') && !left.is_empty())
        .ok_or_else(|| format!("image reference needs host/name:tag, got {raw}"))?;
    let (host, repository) = path
        .split_once('/')
        .ok_or_else(|| format!("image reference needs host/name:tag, got {raw}"))?;
    if host.is_empty() || repository.is_empty() || tag.is_empty() {
        return Err(format!("image reference needs host/name:tag, got {raw}"));
    }
    Ok(ImageRef {
        host: host.to_string(),
        repository: repository.to_string(),
        tag: tag.to_string(),
        tls,
    })
}

fn registry_auth() -> Option<(String, String)> {
    let user = std::env::var("XO_REGISTRY_USER").ok()?;
    let pass = std::env::var("XO_REGISTRY_PASSWORD").ok()?;
    Some((user, pass))
}

fn put_blob(
    dest: &ImageRef,
    layout: &Path,
    digest: &str,
    auth: &Option<(String, String)>,
) -> Result<(), String> {
    let hex = digest_hex(digest)?;
    let bytes = read_blob(layout, hex)?;
    if head_ok(
        dest,
        &format!("/v2/{}/blobs/{}", dest.repository, digest),
        auth,
    )? {
        return Ok(());
    }
    let loc = post_upload(dest, auth)?;
    let sep = if loc.contains('?') { "&" } else { "?" };
    let url_path = if loc.starts_with("http") {
        loc
    } else {
        format!("{}{}{loc}", dest.scheme(), dest.host)
    };
    let put_url = format!("{url_path}{sep}digest={digest}");
    http_put(&put_url, dest.tls, auth, "application/octet-stream", &bytes)?;
    Ok(())
}

impl ImageRef {
    fn scheme(&self) -> &'static str {
        if self.tls { "https://" } else { "http://" }
    }
}

fn put_manifest(
    dest: &ImageRef,
    manifest: &[u8],
    auth: &Option<(String, String)>,
) -> Result<(), String> {
    let path = format!("/v2/{}/manifests/{}", dest.repository, dest.tag);
    let url = format!("{}{}{path}", dest.scheme(), dest.host);
    http_put(
        &url,
        dest.tls,
        auth,
        "application/vnd.oci.image.manifest.v1+json",
        manifest,
    )
}

fn post_upload(dest: &ImageRef, auth: &Option<(String, String)>) -> Result<String, String> {
    let path = format!("/v2/{}/blobs/uploads/", dest.repository);
    let url = format!("{}{}{path}", dest.scheme(), dest.host);
    let resp = http_request("POST", &url, dest.tls, auth, None, &[])?;
    resp.headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("location"))
        .map(|(_, v)| v.clone())
        .ok_or_else(|| "registry upload POST missing Location".to_string())
}

fn head_ok(dest: &ImageRef, path: &str, auth: &Option<(String, String)>) -> Result<bool, String> {
    let url = format!("{}{}{path}", dest.scheme(), dest.host);
    let resp = http_request("HEAD", &url, dest.tls, auth, None, &[])?;
    Ok((200..300).contains(&resp.status))
}

struct HttpResp {
    status: u16,
    headers: Vec<(String, String)>,
}

fn http_put(
    url: &str,
    tls: bool,
    auth: &Option<(String, String)>,
    content_type: &str,
    body: &[u8],
) -> Result<(), String> {
    let resp = http_request("PUT", url, tls, auth, Some(content_type), body)?;
    if (200..300).contains(&resp.status) {
        Ok(())
    } else {
        Err(format!("registry PUT {url} → HTTP {}", resp.status))
    }
}

fn http_request(
    method: &str,
    url: &str,
    tls: bool,
    auth: &Option<(String, String)>,
    content_type: Option<&str>,
    body: &[u8],
) -> Result<HttpResp, String> {
    let _ = tls;
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| format!("bad url {url}"))?;
    let (hostport, path) = rest
        .split_once('/')
        .map(|(h, p)| (h, format!("/{p}")))
        .unwrap_or((rest, "/".into()));
    let use_tls = scheme == "https";
    let (host, port) = split_host_port(hostport, if use_tls { 443 } else { 80 })?;
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {hostport}\r\nConnection: close\r\n");
    if let Some((u, p)) = auth {
        req.push_str(&format!(
            "Authorization: Basic {}\r\n",
            b64(&format!("{u}:{p}"))
        ));
    }
    if let Some(ct) = content_type {
        req.push_str(&format!("Content-Type: {ct}\r\n"));
    }
    req.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    let mut msg = req.into_bytes();
    msg.extend_from_slice(body);

    let raw = if use_tls {
        tls_roundtrip(&host, port, &msg)?
    } else {
        let mut stream = TcpStream::connect((host.as_str(), port))
            .map_err(|e| format!("connect {host}:{port}: {e}"))?;
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .ok();
        stream
            .set_write_timeout(Some(std::time::Duration::from_secs(5)))
            .ok();
        stream.write_all(&msg).map_err(|e| format!("write: {e}"))?;
        let mut buf = Vec::new();
        stream
            .read_to_end(&mut buf)
            .map_err(|e| format!("read: {e}"))?;
        buf
    };
    parse_http_response(&raw)
}

fn tls_roundtrip(host: &str, port: u16, msg: &[u8]) -> Result<Vec<u8>, String> {
    use rustls::pki_types::ServerName;
    use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
    static CRYPTO: std::sync::Once = std::sync::Once::new();
    CRYPTO.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let cfg = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = ServerName::try_from(host.to_string()).map_err(|e| format!("sni {host}: {e}"))?;
    let conn =
        ClientConnection::new(std::sync::Arc::new(cfg), name).map_err(|e| format!("tls: {e}"))?;
    let sock =
        TcpStream::connect((host, port)).map_err(|e| format!("connect {host}:{port}: {e}"))?;
    let mut stream = StreamOwned::new(conn, sock);
    stream
        .write_all(msg)
        .map_err(|e| format!("tls write: {e}"))?;
    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .map_err(|e| format!("tls read: {e}"))?;
    Ok(buf)
}

fn parse_http_response(raw: &[u8]) -> Result<HttpResp, String> {
    let text = String::from_utf8_lossy(raw);
    let head = text.split("\r\n\r\n").next().unwrap_or("");
    let mut lines = head.lines();
    let status_line = lines.next().unwrap_or("");
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if status == 401 {
        return Err("registry returned 401 (set XO_REGISTRY_USER / XO_REGISTRY_PASSWORD)".into());
    }
    let mut headers = Vec::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    Ok(HttpResp { status, headers })
}

fn read_blob(layout: &Path, hex: &str) -> Result<Vec<u8>, String> {
    let p = layout.join("blobs").join("sha256").join(hex);
    fs::read(&p).map_err(|e| format!("blob {}: {e}", p.display()))
}

fn write_blob(dir: &Path, hex: &str, bytes: &[u8]) -> Result<(), String> {
    fs::write(dir.join(hex), bytes).map_err(|e| format!("blob {hex}: {e}"))
}

/// ELF `e_machine` → OCI architecture. v0: 386, amd64, arm64.
fn elf_oci_arch(bytes: &[u8]) -> Result<&'static str, String> {
    if bytes.len() < 20 {
        return Err("ELF too short to read e_machine".into());
    }
    if bytes[5] != 1 {
        return Err("big-endian ELF is not a v0 image platform".into());
    }
    let machine = u16::from_le_bytes([bytes[18], bytes[19]]);
    match machine {
        3 => Ok("386"),
        62 => Ok("amd64"),
        183 => Ok("arm64"),
        other => Err(format!("unsupported ELF machine {other} for OCI platform")),
    }
}

/// Split `host`, `host:port`, or `[ipv6]:port` into (host, port).
fn split_host_port(hostport: &str, default_port: u16) -> Result<(String, u16), String> {
    if let Some(rest) = hostport.strip_prefix('[') {
        if let Some((h, p)) = rest.split_once("]:") {
            let port = p
                .parse::<u16>()
                .map_err(|_| format!("bad port in {hostport}"))?;
            return Ok((h.to_string(), port));
        }
        if let Some(h) = rest.strip_suffix(']') {
            return Ok((h.to_string(), default_port));
        }
        return Err(format!("bad ipv6 host {hostport}"));
    }
    if let Some((h, p)) = hostport.rsplit_once(':') {
        if !h.contains(':') && !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) {
            let port = p
                .parse::<u16>()
                .map_err(|_| format!("bad port in {hostport}"))?;
            return Ok((h.to_string(), port));
        }
    }
    Ok((hostport.to_string(), default_port))
}

fn digest_hex(digest: &str) -> Result<&str, String> {
    digest
        .strip_prefix("sha256:")
        .ok_or_else(|| format!("expected sha256: digest, got {digest}"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(bytes).expect("gzip write");
    enc.finish().expect("gzip finish")
}

fn json_string_field(json: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\":\"");
    let i = json.find(&pat)?;
    let rest = &json[i + pat.len()..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn last_json_string_field(json: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\":\"");
    let i = json.rfind(&pat)?;
    let rest = &json[i + pat.len()..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn b64(s: &str) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let b = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        let n = match b.len() - i {
            1 => 1,
            2 => 2,
            _ => 3,
        };
        let x = (b[i] as u32) << 16
            | (if n > 1 { b[i + 1] as u32 } else { 0 }) << 8
            | (if n > 2 { b[i + 2] as u32 } else { 0 });
        out.push(T[((x >> 18) & 63) as usize] as char);
        out.push(T[((x >> 12) & 63) as usize] as char);
        out.push(if n > 1 {
            T[((x >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if n > 2 {
            T[(x & 63) as usize] as char
        } else {
            '='
        });
        i += n;
    }
    out
}

/// Minimal USTAR regular file (absolute path stored without leading slash).
fn ustar_regular_file(path: &str, mode: u32, data: &[u8]) -> Result<Vec<u8>, String> {
    let name = path.trim_start_matches('/');
    if name.len() > 100 {
        return Err("ustar name longer than 100 bytes".into());
    }
    let mut hdr = [0u8; 512];
    hdr[..name.len()].copy_from_slice(name.as_bytes());
    write_octal(&mut hdr[100..108], mode as u64);
    write_octal(&mut hdr[108..116], 0);
    write_octal(&mut hdr[116..124], 0);
    write_octal(&mut hdr[124..136], data.len() as u64);
    write_octal(&mut hdr[136..148], 0);
    hdr[156] = b'0';
    hdr[257..262].copy_from_slice(b"ustar");
    hdr[263] = b'0';
    hdr[264] = b'0';
    // checksum: sum of header with checksum field as spaces
    hdr[148..156].fill(b' ');
    let sum: u32 = hdr.iter().map(|b| *b as u32).sum();
    let cks = format!("{sum:06o}\0 ");
    hdr[148..156].copy_from_slice(cks.as_bytes());

    let mut out = Vec::new();
    out.extend_from_slice(&hdr);
    out.extend_from_slice(data);
    let pad = (512 - (data.len() % 512)) % 512;
    out.extend(std::iter::repeat_n(0u8, pad));
    out.extend_from_slice(&[0u8; 1024]);
    Ok(out)
}

fn write_octal(dst: &mut [u8], value: u64) {
    let width = dst.len().saturating_sub(1);
    let s = format!("{value:0width$o}");
    let bytes = s.as_bytes();
    let n = bytes.len().min(width);
    dst[..n].copy_from_slice(&bytes[..n]);
    dst[width] = 0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn temp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "echo-oci-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn stub_elf(machine: u16) -> Vec<u8> {
        let mut elf = vec![0u8; 64];
        elf[0..4].copy_from_slice(b"\x7fELF");
        elf[4] = 2;
        elf[5] = 1;
        elf[18..20].copy_from_slice(&machine.to_le_bytes());
        elf
    }

    #[test]
    fn scratch_layout_one_app_layer() {
        let dir = temp("layout");
        let bin = dir.join("prog");
        fs::write(&bin, stub_elf(62)).unwrap();
        let dest = dir.join("img");
        write_scratch_image(&bin, &dest).unwrap();
        assert_eq!(
            fs::read_to_string(dest.join("oci-layout")).unwrap(),
            r#"{"imageLayoutVersion":"1.0.0"}"#
        );
        let index = fs::read_to_string(dest.join("index.json")).unwrap();
        assert!(index.contains("linux"));
        assert!(index.contains("amd64"));
        let blobs: Vec<_> = fs::read_dir(dest.join("blobs/sha256"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(blobs.len(), 3);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scratch_layout_arm64_from_elf() {
        let dir = temp("arm64");
        let bin = dir.join("prog");
        fs::write(&bin, stub_elf(183)).unwrap();
        let dest = dir.join("img");
        write_scratch_image(&bin, &dest).unwrap();
        let index = fs::read_to_string(dest.join("index.json")).unwrap();
        assert!(index.contains("arm64"));
        assert!(!index.contains("amd64"));
        let blobs: Vec<_> = fs::read_dir(dest.join("blobs/sha256"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(blobs.len(), 3);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_ref_requires_tag() {
        let r = parse_reference("registry.example.com/acme/app:dev").unwrap();
        assert_eq!(r.host, "registry.example.com");
        assert_eq!(r.repository, "acme/app");
        assert_eq!(r.tag, "dev");
        let with_port = parse_reference("localhost:5000/app:tag").unwrap();
        assert_eq!(with_port.host, "localhost:5000");
        assert_eq!(with_port.repository, "app");
        assert!(parse_reference("nope").is_err());
    }

    #[test]
    fn host_port_splits_hostname() {
        assert_eq!(
            split_host_port("localhost:5000", 80).unwrap(),
            ("localhost".into(), 5000)
        );
        assert_eq!(
            split_host_port("127.0.0.1:1234", 80).unwrap(),
            ("127.0.0.1".into(), 1234)
        );
        assert_eq!(
            split_host_port("registry.example.com", 443).unwrap(),
            ("registry.example.com".into(), 443)
        );
        assert_eq!(
            split_host_port("[::1]:5000", 80).unwrap(),
            ("::1".into(), 5000)
        );
    }

    #[test]
    fn image_platform_follows_elf_machine() {
        assert_eq!(elf_oci_arch(&stub_elf(62)).unwrap(), "amd64");
        assert_eq!(elf_oci_arch(&stub_elf(183)).unwrap(), "arm64");
        assert!(elf_oci_arch(&stub_elf(0)).is_err());
    }

    #[test]
    fn ustar_roundtrip_name() {
        let t = ustar_regular_file("/app", 0o755, b"hi").unwrap();
        assert_eq!(&t[0..3], b"app");
        assert_eq!(t[156], b'0');
    }

    #[test]
    fn push_to_fake_http_registry() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let h = thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            for _ in 0..32 {
                let Ok((mut s, _)) = (loop {
                    match listener.accept() {
                        Ok(pair) => break Ok(pair),
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            if std::time::Instant::now() >= deadline {
                                return;
                            }
                            thread::sleep(std::time::Duration::from_millis(5));
                            continue;
                        }
                        Err(e) => break Err(e),
                    }
                }) else {
                    break;
                };
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                let req = String::from_utf8_lossy(&buf);
                if req.starts_with("HEAD") {
                    let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                } else if req.starts_with("POST") {
                    let _ = s.write_all(
                        b"HTTP/1.1 202 Accepted\r\nLocation: /v2/app/blobs/uploads/1\r\nContent-Length: 0\r\n\r\n",
                    );
                } else {
                    let _ = s.write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n");
                }
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
        });

        let dir = temp("push");
        let bin = dir.join("prog");
        fs::write(&bin, stub_elf(62)).unwrap();
        let dest = dir.join("img");
        write_scratch_image(&bin, &dest).unwrap();
        let r = push_layout(&dest, &format!("http://127.0.0.1:{}/app:tag", addr.port()));
        let _ = fs::remove_dir_all(&dir);
        assert!(r.is_ok(), "{r:?}");
        let _ = h.join();
    }
}
