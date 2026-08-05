//! Embeds the project template bundle.
//!
//! `cargo miden new` needs templates offline, and it needs to be able to say
//! *which* templates it has, so the bundle ships inside the binary along with
//! its digest. A compiler release proves its embedded bundle is byte-identical
//! to the released `templates/v*` archive by comparing that digest.
//!
//! The archive is committed at `templates.tar.gz` rather than generated here
//! from `extra/templates`. That is not a preference: this crate is published,
//! and a `.crate` cannot contain files from outside the package directory, so
//! a build script reading `../../extra/templates` would work in this repository
//! and fail for everyone building from crates.io.
//!
//! The archive is produced by `release-tool bundle`, and `release-tool lint`
//! fails if it drifts from the template sources — which is what keeps a
//! committed artifact honest.

use std::{
    fs,
    path::{Path, PathBuf},
};

fn main() {
    let archive = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates.tar.gz");
    println!("cargo:rerun-if-changed={}", archive.display());
    println!("cargo:rerun-if-changed=build.rs");

    let bytes = fs::read(&archive).unwrap_or_else(|err| {
        panic!(
            "failed to read the embedded template bundle at {}: {err}\n\nRegenerate it with \
             `release-tool bundle --output tools/cargo-miden/templates.tar.gz`.",
            archive.display()
        )
    });

    let digest = sha256_hex(&bytes);
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    fs::write(
        out.join("template_bundle.rs"),
        format!(
            "/// SHA-256 of the embedded template bundle.\n///\n/// A compiler release checks \
             this against the digest of the\n/// `templates/v*` archive it is released \
             beside.\npub const TEMPLATE_BUNDLE_SHA256: &str = \"{digest}\";\n\n/// Size of the \
             embedded bundle, in bytes.\npub const TEMPLATE_BUNDLE_LEN: usize = {};\n",
            bytes.len()
        ),
    )
    .expect("failed to write the generated bundle constants");
}

/// SHA-256, inline to keep the build script dependency-free.
fn sha256_hex(data: &[u8]) -> String {
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

    h.iter()
        .flat_map(|word| word.to_be_bytes())
        .map(|b| format!("{b:02x}"))
        .collect()
}
