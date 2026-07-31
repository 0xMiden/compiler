//! A crates.io-shaped registry for release rehearsals.
//!
//! It serves a sparse index for crates published to it and proxies everything
//! else upstream, which is what lets a rehearsal resolve the full third-party
//! dependency closure without mirroring it. That proxying is precisely what
//! `staging.crates.io` could not do, and it is why rehearsals run against this
//! instead.
//!
//! The HTTP layer is hand-written rather than built on a framework because the
//! interesting behavior is the failure modes: delayed index visibility, rate
//! limiting, and truncated responses all need to be produced deliberately.

pub mod client;
mod index;
mod publish;
mod upstream;

use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::{Context, Result};

pub use self::{
    index::{IndexEntry, index_path},
    publish::PublishRequest,
    upstream::{CurlUpstream, NoUpstream, Upstream},
};

/// Injectable failure behavior, for exercising the executor's retry and
/// reconciliation paths.
#[derive(Debug, Default, Clone)]
pub struct Faults {
    /// Reject this many upload attempts with HTTP 429 before accepting any.
    pub rate_limit_uploads: u32,
    /// Withhold a published version from the index for this many lookups,
    /// simulating propagation delay.
    pub delay_index_visibility: u32,
}

#[derive(Debug, Default)]
pub struct Stats {
    pub published: AtomicU64,
    pub local_index_hits: AtomicU64,
    pub upstream_index_hits: AtomicU64,
    pub rate_limited: AtomicU64,
}

struct State {
    /// Published index entries, keyed by crate name, in publication order.
    entries: BTreeMap<String, Vec<IndexEntry>>,
    /// Stored `.crate` archives, keyed by `<name>-<version>`.
    archives: BTreeMap<String, Vec<u8>>,
    /// Remaining lookups to withhold, keyed by crate name.
    withheld: BTreeMap<String, u32>,
    remaining_rate_limits: u32,
}

/// A running registry. Dropping the handle stops accepting new connections.
pub struct Registry {
    addr: SocketAddr,
    state: Arc<Mutex<State>>,
    stats: Arc<Stats>,
}

impl Registry {
    /// Bind to `127.0.0.1:port` (use port 0 for an ephemeral port) and serve in
    /// a background thread.
    pub fn start(port: u16, faults: Faults, upstream: Arc<dyn Upstream>) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port))
            .with_context(|| format!("failed to bind rehearsal registry on port {port}"))?;
        let addr = listener.local_addr()?;

        let state = Arc::new(Mutex::new(State {
            entries: BTreeMap::new(),
            archives: BTreeMap::new(),
            withheld: BTreeMap::new(),
            remaining_rate_limits: faults.rate_limit_uploads,
        }));
        let stats = Arc::new(Stats::default());

        let server = Server {
            addr,
            state: Arc::clone(&state),
            stats: Arc::clone(&stats),
            faults,
            upstream,
        };

        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let server = server.clone();
                std::thread::spawn(move || {
                    let _ = server.serve(stream);
                });
            }
        });

        Ok(Self { addr, state, stats })
    }

    /// The sparse index URL to hand to Cargo via `--index`.
    pub fn index_url(&self) -> String {
        format!("sparse+http://{}/", self.addr)
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    /// The versions published for a crate, in publication order.
    pub fn published_versions(&self, name: &str) -> Vec<String> {
        self.state
            .lock()
            .expect("registry state is not poisoned")
            .entries
            .get(name)
            .map(|entries| entries.iter().map(|e| e.vers.clone()).collect())
            .unwrap_or_default()
    }

    /// The stored archive for a published version, if any.
    pub fn archive(&self, name: &str, version: &str) -> Option<Vec<u8>> {
        self.state
            .lock()
            .expect("registry state is not poisoned")
            .archives
            .get(&format!("{name}-{version}"))
            .cloned()
    }
}

#[derive(Clone)]
struct Server {
    addr: SocketAddr,
    state: Arc<Mutex<State>>,
    stats: Arc<Stats>,
    faults: Faults,
    upstream: Arc<dyn Upstream>,
}

struct Request {
    method: String,
    path: String,
    body: Vec<u8>,
    /// Whether the client asked for the connection to be closed after the
    /// response. Ignoring this leaves well-behaved clients blocked on a read
    /// that never completes.
    close: bool,
}

impl Server {
    fn serve(&self, stream: TcpStream) -> Result<()> {
        // One buffered reader for the life of the connection. Creating a new one
        // per request would discard bytes already buffered from a pipelined or
        // keep-alive request, which stalls the connection.
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut writer = stream;

        while let Some(request) = read_request(&mut reader)? {
            let (status, content_type, body) = self.dispatch(&request);
            write_response(&mut writer, status, content_type, &body, request.close)?;
            if request.close {
                break;
            }
        }
        Ok(())
    }

    fn dispatch(&self, request: &Request) -> (u16, &'static str, Vec<u8>) {
        let path = request.path.split('?').next().unwrap_or("").to_string();

        if request.method == "PUT" && path.starts_with("/api/v1/crates/new") {
            return self.handle_publish(&request.body);
        }
        if request.method != "GET" {
            return (405, "application/json", b"{\"errors\":[]}".to_vec());
        }
        if path == "/config.json" {
            let config = format!(
                "{{\"dl\":\"http://{addr}/api/v1/crates\",\"api\":\"http://{addr}\"}}",
                addr = self.addr
            );
            return (200, "application/json", config.into_bytes());
        }
        if let Some(rest) = path.strip_prefix("/api/v1/crates/") {
            // `<name>/<version>/download`
            let parts: Vec<&str> = rest.split('/').collect();
            if parts.len() == 3 && parts[2] == "download" {
                return self.handle_download(parts[0], parts[1]);
            }
        }
        self.handle_index(&path)
    }

    fn handle_publish(&self, body: &[u8]) -> (u16, &'static str, Vec<u8>) {
        {
            let mut state = self.state.lock().expect("registry state is not poisoned");
            if state.remaining_rate_limits > 0 {
                state.remaining_rate_limits -= 1;
                self.stats.rate_limited.fetch_add(1, Ordering::Relaxed);
                return (
                    429,
                    "application/json",
                    br#"{"errors":[{"detail":"too many requests"}]}"#.to_vec(),
                );
            }
        }

        let request = match PublishRequest::decode(body) {
            Ok(request) => request,
            Err(err) => {
                let message = format!(r#"{{"errors":[{{"detail":"{err}"}}]}}"#);
                return (400, "application/json", message.into_bytes());
            }
        };

        let cksum = sha256_hex(&request.crate_bytes);
        let entry = request.to_index_entry(cksum);
        let key = format!("{}-{}", entry.name, entry.vers);

        let mut state = self.state.lock().expect("registry state is not poisoned");
        state.archives.insert(key, request.crate_bytes);
        if self.faults.delay_index_visibility > 0 {
            state.withheld.insert(entry.name.clone(), self.faults.delay_index_visibility);
        }
        state.entries.entry(entry.name.clone()).or_default().push(entry);
        drop(state);

        self.stats.published.fetch_add(1, Ordering::Relaxed);
        (200, "application/json", br#"{"warnings":{"other":[]}}"#.to_vec())
    }

    fn handle_download(&self, name: &str, version: &str) -> (u16, &'static str, Vec<u8>) {
        let key = format!("{name}-{version}");
        if let Some(bytes) =
            self.state.lock().expect("registry state is not poisoned").archives.get(&key)
        {
            return (200, "application/octet-stream", bytes.clone());
        }
        match self.upstream.fetch_archive(name, version) {
            Some(bytes) => (200, "application/octet-stream", bytes),
            None => (404, "application/json", b"{\"errors\":[]}".to_vec()),
        }
    }

    fn handle_index(&self, path: &str) -> (u16, &'static str, Vec<u8>) {
        let name = path.rsplit('/').next().unwrap_or("").to_string();
        if name.is_empty() {
            return (404, "application/json", b"{\"errors\":[]}".to_vec());
        }

        {
            let mut state = self.state.lock().expect("registry state is not poisoned");
            let withheld = state.withheld.get(&name).copied().unwrap_or(0);
            if withheld > 0 {
                state.withheld.insert(name.clone(), withheld - 1);
                return (404, "text/plain", Vec::new());
            }
            if let Some(entries) = state.entries.get(&name) {
                self.stats.local_index_hits.fetch_add(1, Ordering::Relaxed);
                let body: String = entries.iter().map(IndexEntry::to_line).collect();
                return (200, "text/plain", body.into_bytes());
            }
        }

        match self.upstream.fetch_index(path) {
            Some(body) => {
                self.stats.upstream_index_hits.fetch_add(1, Ordering::Relaxed);
                (200, "text/plain", body)
            }
            None => (404, "text/plain", Vec::new()),
        }
    }
}

fn read_request(reader: &mut BufReader<TcpStream>) -> Result<Option<Request>> {
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(None); // client closed the connection
    }

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();

    let mut content_length = 0usize;
    let mut close = false;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 {
            break;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            } else if name.eq_ignore_ascii_case("connection") {
                close = value.trim().eq_ignore_ascii_case("close");
            }
        }
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }

    Ok(Some(Request {
        method,
        path,
        body,
        close,
    }))
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
    close: bool,
) -> Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        429 => "Too Many Requests",
        _ => "Unknown",
    };
    let connection = if close { "close" } else { "keep-alive" };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: \
         {}\r\nConnection: {connection}\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}

/// SHA-256, implemented here to keep this tool dependency-free.
pub fn sha256_hex(data: &[u8]) -> String {
    let digest = sha256(data);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let mut message = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in message.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes(word.try_into().expect("4 bytes"));
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }

        let (mut a, mut b, mut c, mut d) = (h[0], h[1], h[2], h[3]);
        let (mut e, mut f, mut g, mut hh) = (h[4], h[5], h[6], h[7]);

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        for (slot, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(value);
        }
    }

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // Longer than one block, to exercise the chunk loop.
        assert_eq!(
            sha256_hex(&b"a".repeat(1000)),
            "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"
        );
    }
}
