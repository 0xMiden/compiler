//! The GitHub REST implementation.
//!
//! Every call goes through `curl`, for the same reasons the registry client
//! does: no TLS stack in the dependency graph, and `curl` is present wherever
//! Actions runs.
//!
//! The base URLs are configurable so the whole surface can be exercised against
//! a local stub. That matters more here than elsewhere — this is the code that
//! creates tags and publishes immutable releases, and the first time it runs
//! against real GitHub should not be the first time its request shapes are
//! tested.
//!
//! Asset uploads go to a *different* host than the rest of the API. GitHub
//! returns that host in each release's `upload_url`, so it is read from the
//! response rather than assumed.

use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::{Asset, GitHub, Release};

const RELEASES_PER_PAGE: usize = 100;

/// Enough pages to cover every release this repository will plausibly have.
/// Bounded rather than unbounded so a paging bug cannot loop forever.
const MAX_RELEASE_PAGES: usize = 20;

pub struct RestGitHub {
    api_base: String,
    /// `owner/repo`.
    repo: String,
    token: String,
    /// Overrides the upload host GitHub advertises. Only a stub needs this.
    upload_base: Option<String>,
}

impl RestGitHub {
    /// Build a client from the environment Actions provides.
    ///
    /// The token is read from `GITHUB_TOKEN` and never logged or passed as a
    /// command-line argument.
    pub fn from_env() -> Result<Self> {
        let repo = std::env::var("GITHUB_REPOSITORY")
            .context("GITHUB_REPOSITORY is not set; this expects to run under Actions")?;
        let token = std::env::var("GITHUB_TOKEN")
            .context("GITHUB_TOKEN is not set; GitHub operations need a token")?;
        Ok(Self {
            api_base: std::env::var("GITHUB_API_URL")
                .unwrap_or_else(|_| "https://api.github.com".to_string()),
            repo,
            token,
            upload_base: None,
        })
    }

    /// A client pointed at a stub.
    pub fn for_testing(api_base: impl Into<String>, repo: impl Into<String>) -> Self {
        let api_base = api_base.into();
        Self {
            upload_base: Some(api_base.clone()),
            api_base,
            repo: repo.into(),
            token: "stub-token".to_string(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/repos/{}{}", self.api_base.trim_end_matches('/'), self.repo, path)
    }

    /// Issue a request, returning the status and body.
    ///
    /// Statuses are returned rather than turned into errors, because several
    /// callers need to distinguish "absent" from "failed" — a 404 for a tag is
    /// an answer, not a problem.
    fn request(&self, method: &str, url: &str, body: Option<Body<'_>>) -> Result<(u16, Vec<u8>)> {
        let mut headers = format!(
            "Authorization: Bearer {}\nAccept: application/vnd.github+json\nX-GitHub-Api-Version: \
             2022-11-28\nUser-Agent: midenc-release\n",
            self.token
        );
        let payload = match &body {
            Some(Body::Json(json)) => {
                headers.push_str("Content-Type: application/json\n");
                Some(json.as_bytes().to_vec())
            }
            Some(Body::Binary(bytes)) => {
                headers.push_str("Content-Type: application/octet-stream\n");
                Some(bytes.to_vec())
            }
            None => None,
        };

        let mut command = Command::new("curl");
        command
            .args(["--silent", "--show-error", "--location", "--max-time", "120"])
            .args(["--write-out", "\n%{http_code}"])
            .args(["-X", method]);

        // curl reads `@-` from stdin, and only one argument may do so. The
        // authorization header must not go in argv, which is world-readable, so
        // when there is a body the headers move to a temporary file and stdin
        // carries the body.
        let header_file = match &payload {
            None => {
                command.args(["--header", "@-"]);
                None
            }
            Some(_) => {
                let path = std::env::temp_dir().join(format!(
                    "midenc-release-headers-{}-{:?}",
                    std::process::id(),
                    std::thread::current().id()
                ));
                std::fs::write(&path, &headers)?;
                command.args(["--header", &format!("@{}", path.display())]);
                command.args(["--data-binary", "@-"]);
                Some(path)
            }
        };
        command.arg(url);

        let stdin = payload.unwrap_or_else(|| headers.as_bytes().to_vec());
        let output = run_with_stdin(&mut command, &stdin);

        if let Some(path) = header_file {
            let _ = std::fs::remove_file(path);
        }
        let output = output?;

        if !output.status.success() {
            bail!("{method} {url} failed: {}", String::from_utf8_lossy(&output.stderr).trim());
        }

        let (status, body) = split_status(output.stdout)?;
        Ok((status, body))
    }
}

/// Split curl's `--write-out` status line off the end of a response.
fn split_status(mut stdout: Vec<u8>) -> Result<(u16, Vec<u8>)> {
    let split = stdout
        .iter()
        .rposition(|byte| *byte == b'\n')
        .context("curl produced no status line")?;
    let status: u16 = String::from_utf8_lossy(&stdout[split + 1..])
        .trim()
        .parse()
        .context("curl produced an unparsable status")?;
    stdout.truncate(split);
    Ok((status, stdout))
}

enum Body<'a> {
    Json(String),
    Binary(&'a [u8]),
}

fn run_with_stdin(command: &mut Command, stdin: &[u8]) -> Result<std::process::Output> {
    use std::io::Write;
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to run curl")?;
    child.stdin.as_mut().expect("stdin was piped").write_all(stdin)?;
    Ok(child.wait_with_output()?)
}

#[derive(Debug, Deserialize)]
struct ReleaseResponse {
    id: u64,
    tag_name: String,
    draft: bool,
    prerelease: bool,
    #[serde(default)]
    target_commitish: String,
    #[serde(default)]
    upload_url: String,
}

#[derive(Debug, Deserialize)]
struct AssetResponse {
    id: u64,
    name: String,
    size: usize,
}

#[derive(Debug, Deserialize)]
struct RefResponse {
    object: RefObject,
}

#[derive(Debug, Deserialize)]
struct RefObject {
    sha: String,
}

impl From<ReleaseResponse> for Release {
    fn from(response: ReleaseResponse) -> Self {
        Self {
            id: response.id,
            tag: response.tag_name,
            draft: response.draft,
            prerelease: response.prerelease,
            target_commitish: response.target_commitish,
        }
    }
}

impl RestGitHub {
    fn release(&self, id: u64) -> Result<ReleaseResponse> {
        let (status, body) = self.request("GET", &self.url(&format!("/releases/{id}")), None)?;
        if status != 200 {
            bail!("failed to read release {id}: HTTP {status}");
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// The host assets are uploaded to, taken from the release's `upload_url`.
    fn upload_url(&self, release: &ReleaseResponse, name: &str) -> String {
        if let Some(base) = &self.upload_base {
            return format!(
                "{}/repos/{}/releases/{}/assets?name={name}",
                base.trim_end_matches('/'),
                self.repo,
                release.id
            );
        }
        // GitHub advertises a URI template: `https://.../assets{?name,label}`.
        let base = release.upload_url.split('{').next().unwrap_or(&release.upload_url);
        format!("{base}?name={name}")
    }

    fn asset_id(&self, release: u64, name: &str) -> Result<u64> {
        let assets = self.asset_responses(release)?;
        assets
            .into_iter()
            .find(|asset| asset.name == name)
            .map(|asset| asset.id)
            .with_context(|| format!("release {release} has no asset '{name}'"))
    }

    /// Delete an asset already attached to a release, if one exists by that name.
    ///
    /// Only ever called on a draft: published releases are immutable, and
    /// nothing in the flow uploads to one.
    fn remove_asset_if_present(&self, release: u64, name: &str) -> Result<()> {
        let Some(existing) =
            self.asset_responses(release)?.into_iter().find(|asset| asset.name == name)
        else {
            return Ok(());
        };

        let (status, body) =
            self.request("DELETE", &self.url(&format!("/releases/assets/{}", existing.id)), None)?;
        // 204 is success; 404 means someone else removed it, which is the state
        // we wanted anyway.
        if status != 204 && status != 404 {
            bail!(
                "failed to replace the existing asset '{name}': HTTP {status}: {}",
                String::from_utf8_lossy(&body).trim()
            );
        }
        Ok(())
    }

    fn asset_responses(&self, release: u64) -> Result<Vec<AssetResponse>> {
        let (status, body) =
            self.request("GET", &self.url(&format!("/releases/{release}/assets")), None)?;
        if status != 200 {
            bail!("failed to list assets for release {release}: HTTP {status}");
        }
        Ok(serde_json::from_slice(&body)?)
    }
}

impl GitHub for RestGitHub {
    fn create_tag(&self, tag: &str, commit: &str) -> Result<()> {
        // `POST /git/refs` fails when the ref exists, which is the behaviour the
        // release depends on: silently moving a tag would be unrecoverable.
        let payload = serde_json::json!({ "ref": format!("refs/tags/{tag}"), "sha": commit });
        let (status, body) =
            self.request("POST", &self.url("/git/refs"), Some(Body::Json(payload.to_string())))?;

        match status {
            201 => Ok(()),
            _ => bail!(
                "failed to create tag '{tag}': HTTP {status}: {}",
                String::from_utf8_lossy(&body).trim()
            ),
        }
    }

    fn tag_commit(&self, tag: &str) -> Result<Option<String>> {
        let (status, body) =
            self.request("GET", &self.url(&format!("/git/ref/tags/{tag}")), None)?;
        match status {
            200 => {
                let parsed: RefResponse = serde_json::from_slice(&body)?;
                Ok(Some(parsed.object.sha))
            }
            404 => Ok(None),
            _ => bail!("failed to read tag '{tag}': HTTP {status}"),
        }
    }

    fn create_draft(&self, tag: &str, commit: &str, prerelease: bool) -> Result<Release> {
        let payload = serde_json::json!({
            "tag_name": tag,
            "target_commitish": commit,
            "draft": true,
            "prerelease": prerelease,
            // A release is never made `latest` on creation; that is decided when
            // the draft is published, and only stable compiler releases qualify.
            "make_latest": "false",
        });
        let (status, body) =
            self.request("POST", &self.url("/releases"), Some(Body::Json(payload.to_string())))?;
        if status != 201 {
            bail!(
                "failed to create a draft for '{tag}': HTTP {status}: {}",
                String::from_utf8_lossy(&body).trim()
            );
        }
        let parsed: ReleaseResponse = serde_json::from_slice(&body)?;
        Ok(parsed.into())
    }

    /// Find a release by tag, **including drafts**.
    ///
    /// `GET /releases/tags/{tag}` cannot be used here: it returns only published
    /// releases and 404s for a draft, even when the tag itself exists. A draft
    /// is exactly what this needs to find — it is what staging creates, what a
    /// resume must reuse rather than duplicate, what `discard` deletes, and what
    /// finalization publishes. Using that endpoint meant every run created a
    /// second draft for the same tag, `discard` silently deleted nothing, and
    /// finalization reported that staging had not completed.
    ///
    /// Listing is therefore the only correct source. A published release wins
    /// over a draft for the same tag, so staging still refuses to modify one.
    fn release_by_tag(&self, tag: &str) -> Result<Option<Release>> {
        let mut candidates: Vec<Release> = Vec::new();

        for page in 1..=MAX_RELEASE_PAGES {
            let (status, body) = self.request(
                "GET",
                &self.url(&format!("/releases?per_page={RELEASES_PER_PAGE}&page={page}")),
                None,
            )?;
            if status != 200 {
                bail!("failed to list releases while looking for '{tag}': HTTP {status}");
            }
            let parsed: Vec<ReleaseResponse> = serde_json::from_slice(&body)?;
            let count = parsed.len();

            candidates
                .extend(parsed.into_iter().map(Release::from).filter(|release| release.tag == tag));

            // A short page is the last page.
            if count < RELEASES_PER_PAGE {
                break;
            }
        }

        // A published release is authoritative: staging must see it and refuse,
        // rather than finding a leftover draft beside it and populating that.
        candidates.sort_by_key(|release| release.draft);
        Ok(candidates.into_iter().next())
    }

    fn upload_asset(&self, release: u64, name: &str, bytes: &[u8]) -> Result<Asset> {
        let response = self.release(release)?;
        let url = self.upload_url(&response, name);

        // An asset name is unique within a release, and uploading over one is
        // rejected rather than replaced. A resume re-stages the same draft with
        // freshly built bytes -- binary builds are not required to be
        // bit-for-bit reproducible -- so the existing asset is removed first.
        // Doing this only on conflict would leave the first attempt's bytes in
        // place whenever they happen to match by name.
        self.remove_asset_if_present(release, name)?;

        let (status, body) = self.request("POST", &url, Some(Body::Binary(bytes)))?;
        if status != 201 {
            bail!(
                "failed to upload '{name}': HTTP {status}: {}",
                String::from_utf8_lossy(&body).trim()
            );
        }
        Ok(Asset {
            name: name.to_string(),
            size: bytes.len(),
            digest: crate::registry::sha256_hex(bytes),
        })
    }

    fn assets(&self, release: u64) -> Result<Vec<Asset>> {
        let mut assets = Vec::new();
        for response in self.asset_responses(release)? {
            // The digest is computed from the bytes rather than trusted from
            // metadata, since verifying an upload is the entire point.
            let bytes = self.download_asset(release, &response.name)?;
            assets.push(Asset {
                name: response.name,
                size: response.size,
                digest: crate::registry::sha256_hex(&bytes),
            });
        }
        Ok(assets)
    }

    fn download_asset(&self, release: u64, name: &str) -> Result<Vec<u8>> {
        let id = self.asset_id(release, name)?;
        // Assets download from the API host, but only with an octet-stream
        // Accept header; the default returns metadata instead.
        let url = self.url(&format!("/releases/assets/{id}"));
        let mut command = Command::new("curl");
        command
            .args(["--silent", "--show-error", "--location", "--max-time", "300"])
            .args(["--write-out", "\n%{http_code}"])
            .args(["--header", "@-"])
            .arg(&url);

        let headers = format!(
            "Authorization: Bearer {}\nAccept: application/octet-stream\nX-GitHub-Api-Version: \
             2022-11-28\nUser-Agent: midenc-release\n",
            self.token
        );
        let output = run_with_stdin(&mut command, headers.as_bytes())?;
        if !output.status.success() {
            bail!("failed to download '{name}'");
        }

        let (status, stdout) = split_status(output.stdout)?;
        if status != 200 {
            bail!("failed to download '{name}': HTTP {status}");
        }
        Ok(stdout)
    }

    fn delete_release(&self, release: u64) -> Result<()> {
        let (status, body) =
            self.request("DELETE", &self.url(&format!("/releases/{release}")), None)?;
        if status != 204 {
            bail!(
                "failed to delete release {release}: HTTP {status}: {}",
                String::from_utf8_lossy(&body).trim()
            );
        }
        Ok(())
    }

    fn publish_release(&self, release: u64, make_latest: bool) -> Result<Release> {
        // `make_latest` is a string in this API, not a boolean: "true", "false",
        // or "legacy". Sending an actual boolean is accepted and ignored, which
        // would silently hand the latest-release slot to whatever published
        // last.
        let payload = serde_json::json!({
            "draft": false,
            "make_latest": if make_latest { "true" } else { "false" },
        });
        let (status, body) = self.request(
            "PATCH",
            &self.url(&format!("/releases/{release}")),
            Some(Body::Json(payload.to_string())),
        )?;
        if status != 200 {
            bail!(
                "failed to publish release {release}: HTTP {status}: {}",
                String::from_utf8_lossy(&body).trim()
            );
        }
        let parsed: ReleaseResponse = serde_json::from_slice(&body)?;
        Ok(parsed.into())
    }
}
