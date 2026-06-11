use miden_base_macros::component_storage;

#[component_storage]
struct Contract {
    counter: u32,
}

fn main() {}
