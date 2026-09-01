//! A minimal `*`/`?` matcher over asset file names.
//!
//! Asset routing needs to answer "does this file name match this pattern",
//! nothing more: `--artifacts` is a flat directory, so there are no path
//! separators to reason about and no character classes worth supporting. A
//! dependency for that would be out of proportion to this tool's deliberately
//! minimal dependency list.

/// Whether `name` matches `pattern`, where `*` matches any run of bytes
/// (including none) and `?` matches exactly one byte.
///
/// Iterative with backtracking rather than recursive: a recursive matcher on
/// `*`-heavy input costs stack depth for no gain.
pub fn matches(pattern: &str, name: &str) -> bool {
    let pattern = pattern.as_bytes();
    let name = name.as_bytes();

    let (mut p, mut n) = (0usize, 0usize);
    // Where to resume if the current `*` turns out to have consumed too little.
    let mut star: Option<(usize, usize)> = None;

    while n < name.len() {
        match pattern.get(p) {
            Some(b'*') => {
                star = Some((p, n));
                p += 1;
            }
            Some(b'?') => {
                p += 1;
                n += 1;
            }
            Some(&byte) if byte == name[n] => {
                p += 1;
                n += 1;
            }
            _ => match star {
                // Give the last `*` one more byte and try again.
                Some((star_p, star_n)) => {
                    p = star_p + 1;
                    n = star_n + 1;
                    star = Some((star_p, n));
                }
                None => return false,
            },
        }
    }

    // Trailing `*`s may still match nothing.
    while pattern.get(p) == Some(&b'*') {
        p += 1;
    }
    p == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_patterns_match_exactly() {
        assert!(matches("templates.tar.gz", "templates.tar.gz"));
        assert!(!matches("templates.tar.gz", "templates.tar.gz.sig"));
        assert!(!matches("templates.tar.gz", "x-templates.tar.gz"));
    }

    #[test]
    fn star_matches_any_run_including_empty() {
        assert!(matches("midenc-*.tar.gz", "midenc-x86_64-unknown-linux-gnu.tar.gz"));
        assert!(matches("midenc-*.tar.gz", "midenc-.tar.gz"));
        assert!(matches("*", "anything"));
        assert!(matches("*", ""));
    }

    /// The case that decides whether this repository's globs are unambiguous:
    /// `midenc-*` must not claim `cargo-miden-*` or `miden-objtool-*`.
    #[test]
    fn a_prefix_glob_does_not_match_a_different_prefix() {
        assert!(!matches("midenc-*.tar.gz", "cargo-miden-aarch64-apple-darwin.tar.gz"));
        assert!(!matches("midenc-*.tar.gz", "miden-objtool-x86_64-unknown-linux-gnu.tar.gz"));
        assert!(matches("cargo-miden-*.tar.gz", "cargo-miden-aarch64-apple-darwin.tar.gz"));
        assert!(matches(
            "miden-objtool-*.tar.gz",
            "miden-objtool-x86_64-unknown-linux-gnu.tar.gz"
        ));
    }

    #[test]
    fn question_matches_exactly_one_byte() {
        assert!(matches("v?.tar.gz", "v1.tar.gz"));
        assert!(!matches("v?.tar.gz", "v10.tar.gz"));
        assert!(!matches("v?.tar.gz", "v.tar.gz"));
    }

    #[test]
    fn multiple_stars_backtrack_correctly() {
        assert!(matches("*-*.tar.gz", "midenc-aarch64.tar.gz"));
        assert!(matches("a*b*c", "abc"));
        assert!(matches("a*b*c", "azzbzzc"));
        assert!(!matches("a*b*c", "azzbzz"));
    }

    #[test]
    fn a_non_ascii_name_does_not_panic() {
        assert!(matches("*.tar.gz", "midenc-\u{00e9}.tar.gz"));
    }
}
