//! `RestGitHub` driven over real HTTP against the stub.
//!
//! These cover what the in-memory double cannot: the actual requests. If a path
//! or status code is wrong, it is wrong here rather than during a release.

use super::{GitHub, TagOutcome, create_tag_idempotent, rest::RestGitHub, stub_server::StubServer};

fn client(server: &StubServer) -> RestGitHub {
    RestGitHub::for_testing(server.base_url(), "owner/repo")
}

#[test]
fn a_tag_is_created_and_read_back() {
    let server = StubServer::start().unwrap();
    let github = client(&server);

    github.create_tag("v1.0.0", "abc123").unwrap();
    assert!(server.tag_exists("v1.0.0"));
    assert_eq!(github.tag_commit("v1.0.0").unwrap().as_deref(), Some("abc123"));
}

#[test]
fn an_absent_tag_is_an_answer_not_an_error() {
    let server = StubServer::start().unwrap();
    assert_eq!(client(&server).tag_commit("v9.9.9").unwrap(), None);
}

#[test]
fn creating_an_existing_tag_fails_rather_than_moving_it() {
    let server = StubServer::start().unwrap();
    server.insert_tag("v1.0.0", "someone-else");

    // The API returns 422 for an existing ref. Silently updating it would be
    // unrecoverable, so this must surface as a failure.
    assert!(client(&server).create_tag("v1.0.0", "abc123").is_err());

    let err = create_tag_idempotent(&client(&server), "v1.0.0", "abc123")
        .unwrap_err()
        .to_string();
    assert!(err.contains("already exists at someone-else"), "{err}");
}

#[test]
fn a_tag_left_by_an_earlier_attempt_is_recognised_as_a_resume() {
    let server = StubServer::start().unwrap();
    server.insert_tag("v1.0.0", "abc123");
    assert_eq!(
        create_tag_idempotent(&client(&server), "v1.0.0", "abc123").unwrap(),
        TagOutcome::AlreadyCorrect
    );
}

#[test]
fn a_draft_carries_assets_and_publishes() {
    let server = StubServer::start().unwrap();
    let github = client(&server);

    let release = github.create_draft("v1.0.0", "abc123", false).unwrap();
    assert!(release.draft);
    assert_eq!(release.target_commitish, "abc123");

    github
        .upload_asset(release.id, "midenc-x86_64.tar.gz", b"binary-bytes")
        .unwrap();
    github.upload_asset(release.id, "SHA256SUMS", b"sums").unwrap();

    // Assets come back by name, with digests computed from the bytes rather
    // than taken from metadata.
    let assets = github.assets(release.id).unwrap();
    assert_eq!(assets.len(), 2);
    assert_eq!(
        github.download_asset(release.id, "midenc-x86_64.tar.gz").unwrap(),
        b"binary-bytes"
    );

    assert_eq!(server.is_draft("v1.0.0"), Some(true));
    let published = github.publish_release(release.id, false).unwrap();
    assert!(!published.draft);
    assert_eq!(server.is_draft("v1.0.0"), Some(false));
}

#[test]
fn upload_and_verify_round_trips_over_http() {
    let server = StubServer::start().unwrap();
    let github = client(&server);
    let release = github.create_draft("v1.0.0", "abc123", false).unwrap();

    let assets = vec![
        ("midenc".to_string(), b"one".to_vec()),
        ("cargo-miden".to_string(), b"two".to_vec()),
    ];
    let verified = super::upload_and_verify(&github, release.id, &assets).unwrap();
    assert_eq!(verified.len(), 2);

    // Digests come from the bytes, so a mismatch between what was uploaded and
    // what comes back is what this is really checking.
    let midenc = verified.iter().find(|a| a.name == "midenc").unwrap();
    assert_eq!(midenc.digest, crate::registry::sha256_hex(b"one"));
    let cargo = verified.iter().find(|a| a.name == "cargo-miden").unwrap();
    assert_eq!(cargo.digest, crate::registry::sha256_hex(b"two"));
}

#[test]
fn a_draft_can_be_deleted_but_a_published_release_cannot() {
    let server = StubServer::start().unwrap();
    let github = client(&server);

    let draft = github.create_draft("v1.0.0", "abc", false).unwrap();
    github.delete_release(draft.id).unwrap();
    assert!(github.release_by_tag("v1.0.0").unwrap().is_none());

    let published = github.create_draft("v2.0.0", "abc", false).unwrap();
    github.publish_release(published.id, false).unwrap();
    let err = github.delete_release(published.id).unwrap_err().to_string();
    assert!(err.contains("403"), "an immutable release must not be deletable: {err}");
}

#[test]
fn a_release_is_found_by_tag() {
    let server = StubServer::start().unwrap();
    let github = client(&server);
    github.create_draft("sdk/v0.14.0", "abc", true).unwrap();

    let found = github.release_by_tag("sdk/v0.14.0").unwrap().unwrap();
    assert!(found.prerelease);
    assert!(github.release_by_tag("sdk/v0.99.0").unwrap().is_none());
}

/// The bug that broke a live release. A draft is invisible to
/// `GET /releases/tags/{tag}` -- GitHub 404s there even when the tag exists --
/// so finding one has to go through listing. Getting this wrong meant every run
/// created a second draft for the same tag, `discard` deleted nothing, and
/// finalization reported that staging had never completed.
#[test]
fn a_draft_is_found_by_tag() {
    let server = StubServer::start().unwrap();
    let github = client(&server);

    let created = github.create_draft("sdk/v0.14.0-rc.1", "abc123", true).unwrap();
    assert!(created.draft);

    let found = github
        .release_by_tag("sdk/v0.14.0-rc.1")
        .unwrap()
        .expect("a draft must be findable by its tag");
    assert_eq!(found.id, created.id);
    assert!(found.draft);
}

/// The observable consequence: staging twice must reuse the draft, not leave two
/// candidates for publication behind.
#[test]
fn staging_the_same_tag_twice_does_not_duplicate_the_draft() {
    use std::collections::BTreeMap;

    use crate::staging;

    let server = StubServer::start().unwrap();
    let github = client(&server);
    let plan = crate::plan::Plan {
        schema_version: 1,
        intent: crate::intent::Intent {
            schema_version: 1,
            subject: "abc123".into(),
            candidate_digest: "cand".into(),
            stages: vec![crate::intent::Stage {
                unit: "sdk".into(),
                version: "0.14.0-rc.1".into(),
                prerelease: true,
                packages: vec![],
            }],
            tags: vec![crate::intent::Tag {
                unit: "sdk".into(),
                name: "sdk/v0.14.0-rc.1".into(),
            }],
        },
        packages: vec![crate::plan::SealedPackage {
            name: "a".into(),
            version: "0.14.0-rc.1".into(),
            digest: "d".into(),
            size: 1,
        }],
    };

    let first = staging::stage(&github, &plan, &BTreeMap::new()).unwrap();
    let second = staging::stage(&github, &plan, &BTreeMap::new()).unwrap();
    assert_eq!(
        first[0].release_id, second[0].release_id,
        "a resume must reuse the draft rather than create a rival one"
    );
}

/// Re-staging uploads the same asset names again. GitHub rejects a duplicate
/// name rather than replacing it, and a rebuilt binary is not required to be
/// byte-identical, so the existing asset is replaced with this run's bytes.
#[test]
fn re_uploading_an_asset_replaces_it() {
    let server = StubServer::start().unwrap();
    let github = client(&server);
    let release = github.create_draft("v1.0.0", "abc123", false).unwrap();

    github.upload_asset(release.id, "midenc.tar.gz", b"first build").unwrap();
    github.upload_asset(release.id, "midenc.tar.gz", b"second build").unwrap();

    let assets = github.assets(release.id).unwrap();
    assert_eq!(assets.len(), 1, "the asset must be replaced, not duplicated: {assets:?}");
    assert_eq!(github.download_asset(release.id, "midenc.tar.gz").unwrap(), b"second build");
}

/// A published release wins over a leftover draft for the same tag, so staging
/// sees the published one and refuses rather than populating the draft beside it.
#[test]
fn a_published_release_takes_precedence_over_a_draft_for_the_same_tag() {
    let server = StubServer::start().unwrap();
    let github = client(&server);

    let draft = github.create_draft("v1.0.0", "abc123", false).unwrap();
    let published = github.create_draft("v1.0.0", "abc123", false).unwrap();
    github.publish_release(published.id, false).unwrap();

    let found = github.release_by_tag("v1.0.0").unwrap().unwrap();
    assert_eq!(found.id, published.id, "the published release is authoritative");
    assert!(!found.draft);
    assert_ne!(found.id, draft.id);
}
