//! A local stand-in for the GitHub REST API.
//!
//! This exists so [`super::rest::RestGitHub`] can be exercised over real HTTP
//! without touching GitHub. The in-memory [`super::StubGitHub`] tests the
//! *logic* built on the trait; this tests the *requests* — paths, methods,
//! status codes, and JSON shapes — which is the part that fails on the first
//! real run if it is wrong.
//!
//! It reproduces GitHub's behaviour where the release flow depends on it: a ref
//! that already exists is a 422, an absent tag is a 404 rather than an error,
//! and a published release cannot be deleted.

use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{Arc, Mutex},
};

use anyhow::Result;

#[derive(Debug, Default)]
struct State {
    next_id: u64,
    tags: BTreeMap<String, String>,
    releases: BTreeMap<u64, ReleaseRecord>,
    assets: BTreeMap<u64, BTreeMap<String, (u64, Vec<u8>)>>,
}

#[derive(Debug, Clone)]
struct ReleaseRecord {
    id: u64,
    tag: String,
    draft: bool,
    prerelease: bool,
    commit: String,
}

pub struct StubServer {
    addr: SocketAddr,
    state: Arc<Mutex<State>>,
}

impl StubServer {
    pub fn start() -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let addr = listener.local_addr()?;
        let state = Arc::new(Mutex::new(State::default()));

        let served = Arc::clone(&state);
        let served_addr = addr;
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let state = Arc::clone(&served);
                std::thread::spawn(move || {
                    let _ = serve(stream, state, served_addr);
                });
            }
        });

        Ok(Self { addr, state })
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn tag_exists(&self, tag: &str) -> bool {
        self.state.lock().unwrap().tags.contains_key(tag)
    }

    /// Pre-create a tag, as a squatter or an earlier attempt would have.
    pub fn insert_tag(&self, tag: &str, commit: &str) {
        self.state.lock().unwrap().tags.insert(tag.into(), commit.into());
    }

    pub fn is_draft(&self, tag: &str) -> Option<bool> {
        self.state
            .lock()
            .unwrap()
            .releases
            .values()
            .find(|r| r.tag == tag)
            .map(|r| r.draft)
    }
}

fn serve(stream: TcpStream, state: Arc<Mutex<State>>, addr: SocketAddr) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;

    loop {
        let mut request_line = String::new();
        if reader.read_line(&mut request_line)? == 0 {
            return Ok(());
        }
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_string();
        let target = parts.next().unwrap_or_default().to_string();

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

        let (status, response) = dispatch(&method, &target, &body, &state, addr);
        let head = format!(
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: \
             {}\r\nConnection: {}\r\n\r\n",
            response.len(),
            if close { "close" } else { "keep-alive" }
        );
        writer.write_all(head.as_bytes())?;
        writer.write_all(&response)?;
        writer.flush()?;

        if close {
            return Ok(());
        }
    }
}

fn dispatch(
    method: &str,
    target: &str,
    body: &[u8],
    state: &Arc<Mutex<State>>,
    addr: SocketAddr,
) -> (u16, Vec<u8>) {
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path, query),
        None => (target, ""),
    };
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    let mut state = state.lock().unwrap();

    // /repos/{owner}/{repo}/...
    let rest = if segments.len() >= 3 && segments[0] == "repos" {
        &segments[3..]
    } else {
        return (404, json_error("not found"));
    };

    match (method, rest) {
        ("POST", ["git", "refs"]) => {
            let payload: serde_json::Value = match serde_json::from_slice(body) {
                Ok(value) => value,
                Err(_) => return (400, json_error("malformed json")),
            };
            let name = payload["ref"].as_str().unwrap_or_default();
            let tag = name.strip_prefix("refs/tags/").unwrap_or(name).to_string();
            let sha = payload["sha"].as_str().unwrap_or_default().to_string();

            if state.tags.contains_key(&tag) {
                // GitHub's response when a ref already exists. The release flow
                // depends on this being a failure rather than an update.
                return (422, json_error("Reference already exists"));
            }
            state.tags.insert(tag, sha.clone());
            (201, serde_json::json!({ "object": { "sha": sha } }).to_string().into_bytes())
        }

        // Release tags contain slashes -- `sdk/v0.14.0`, `templates/v2.0.0` --
        // so the tag is everything after the prefix, not one path segment.
        ("GET", ["git", "ref", "tags", ..]) => match state.tags.get(&rest[3..].join("/")) {
            Some(sha) => {
                (200, serde_json::json!({ "object": { "sha": sha } }).to_string().into_bytes())
            }
            None => (404, json_error("Not Found")),
        },

        ("POST", ["releases"]) => {
            let payload: serde_json::Value = match serde_json::from_slice(body) {
                Ok(value) => value,
                Err(_) => return (400, json_error("malformed json")),
            };
            state.next_id += 1;
            let record = ReleaseRecord {
                id: state.next_id,
                tag: payload["tag_name"].as_str().unwrap_or_default().to_string(),
                draft: payload["draft"].as_bool().unwrap_or(false),
                prerelease: payload["prerelease"].as_bool().unwrap_or(false),
                commit: payload["target_commitish"].as_str().unwrap_or_default().to_string(),
            };
            state.releases.insert(record.id, record.clone());
            (201, release_json(&record, addr).into_bytes())
        }

        // Published releases only. GitHub 404s here for a draft even when the
        // tag exists, because a draft is not reachable by tag name -- which is
        // why nothing in the release flow may use this endpoint to find one.
        ("GET", ["releases", "tags", ..]) => {
            let tag = rest[2..].join("/");
            match state.releases.values().find(|r| r.tag == tag && !r.draft) {
                Some(record) => (200, release_json(record, addr).into_bytes()),
                None => (404, json_error("Not Found")),
            }
        }

        // Listing is the only way to see drafts. Paginated, like the real API.
        ("GET", ["releases"]) => {
            let param = |key: &str| -> Option<usize> {
                query
                    .split('&')
                    .find_map(|pair| pair.strip_prefix(&format!("{key}=")))
                    .and_then(|value| value.parse().ok())
            };
            let per_page = param("per_page").unwrap_or(30).clamp(1, 100);
            let page = param("page").unwrap_or(1).max(1);

            // Newest first, as GitHub orders them.
            let mut all: Vec<&ReleaseRecord> = state.releases.values().collect();
            all.sort_by_key(|record| std::cmp::Reverse(record.id));

            let body: Vec<serde_json::Value> = all
                .into_iter()
                .skip((page - 1) * per_page)
                .take(per_page)
                .map(|record| {
                    serde_json::from_str(&release_json(record, addr)).expect("valid release json")
                })
                .collect();
            (200, serde_json::to_vec(&body).expect("serializable"))
        }

        ("GET", ["releases", id]) => {
            match id.parse::<u64>().ok().and_then(|id| state.releases.get(&id)) {
                Some(record) => (200, release_json(record, addr).into_bytes()),
                None => (404, json_error("Not Found")),
            }
        }

        ("PATCH", ["releases", id]) => {
            let payload: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
            let Some(id) = id.parse::<u64>().ok() else {
                return (404, json_error("Not Found"));
            };
            let Some(record) = state.releases.get_mut(&id) else {
                return (404, json_error("Not Found"));
            };
            if let Some(draft) = payload["draft"].as_bool() {
                record.draft = draft;
            }
            let record = record.clone();
            (200, release_json(&record, addr).into_bytes())
        }

        ("DELETE", ["releases", id]) => {
            let Some(id) = id.parse::<u64>().ok() else {
                return (404, json_error("Not Found"));
            };
            match state.releases.get(&id) {
                // An immutable release is not deletable.
                Some(record) if !record.draft => (403, json_error("release is published")),
                Some(_) => {
                    state.releases.remove(&id);
                    state.assets.remove(&id);
                    (204, Vec::new())
                }
                None => (404, json_error("Not Found")),
            }
        }

        ("POST", ["releases", id, "assets"]) => {
            let Some(id) = id.parse::<u64>().ok() else {
                return (404, json_error("Not Found"));
            };
            if !state.releases.contains_key(&id) {
                return (404, json_error("Not Found"));
            }
            let name = query
                .split('&')
                .find_map(|pair| pair.strip_prefix("name="))
                .unwrap_or("unnamed")
                .to_string();

            // Asset names are unique within a release, and GitHub rejects a
            // second upload rather than replacing the first.
            if state.assets.get(&id).is_some_and(|assets| assets.contains_key(&name)) {
                return (422, json_error("Validation Failed: already_exists"));
            }

            state.next_id += 1;
            let asset_id = state.next_id;
            let size = body.len();
            state
                .assets
                .entry(id)
                .or_default()
                .insert(name.clone(), (asset_id, body.to_vec()));
            (
                201,
                serde_json::json!({ "id": asset_id, "name": name, "size": size })
                    .to_string()
                    .into_bytes(),
            )
        }

        ("GET", ["releases", id, "assets"]) => {
            let Some(id) = id.parse::<u64>().ok() else {
                return (404, json_error("Not Found"));
            };
            let assets: Vec<serde_json::Value> = state
                .assets
                .get(&id)
                .map(|assets| {
                    assets
                        .iter()
                        .map(|(name, (asset_id, bytes))| {
                            serde_json::json!({
                                "id": asset_id, "name": name, "size": bytes.len()
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            (200, serde_json::Value::Array(assets).to_string().into_bytes())
        }

        ("GET", ["releases", "assets", asset_id]) => {
            let Some(wanted) = asset_id.parse::<u64>().ok() else {
                return (404, json_error("Not Found"));
            };
            for assets in state.assets.values() {
                for (id, bytes) in assets.values() {
                    if *id == wanted {
                        return (200, bytes.clone());
                    }
                }
            }
            (404, json_error("Not Found"))
        }

        ("DELETE", ["releases", "assets", asset_id]) => {
            let Some(wanted) = asset_id.parse::<u64>().ok() else {
                return (404, json_error("Not Found"));
            };
            for assets in state.assets.values_mut() {
                if let Some(name) =
                    assets.iter().find(|(_, (id, _))| *id == wanted).map(|(name, _)| name.clone())
                {
                    assets.remove(&name);
                    return (204, Vec::new());
                }
            }
            (404, json_error("Not Found"))
        }

        _ => (404, json_error("not found")),
    }
}

fn release_json(record: &ReleaseRecord, addr: SocketAddr) -> String {
    serde_json::json!({
        "id": record.id,
        "tag_name": record.tag,
        "draft": record.draft,
        "prerelease": record.prerelease,
        "target_commitish": record.commit,
        // GitHub advertises the upload host as a URI template, and the client
        // must strip the template part rather than use the string as-is.
        "upload_url": format!(
            "http://{addr}/repos/owner/repo/releases/{}/assets{{?name,label}}",
            record.id
        ),
    })
    .to_string()
}

fn json_error(message: &str) -> Vec<u8> {
    serde_json::json!({ "message": message }).to_string().into_bytes()
}
