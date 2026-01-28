use miden_base_macros::component;

#[component(foo)]
struct Contract;

fn main() {} // trybuild needs `fn main`
