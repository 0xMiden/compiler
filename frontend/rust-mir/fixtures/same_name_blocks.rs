//! Fixture: two block-local functions with the same display path.

// Nothing calls the helpers. The test calls them directly.
#![allow(dead_code)]

/// Returns `x`. Each block defines its own `helper`, thus both have the path `outer::helper`.
pub fn outer(x: u32) -> u32 {
    let a = {
        fn helper() -> u32 {
            1
        }
        x
    };
    let b = {
        fn helper() -> u32 {
            2
        }
        a
    };
    b
}
