# collatz

## Useful commands

`collatz` is built using the [Miden compiler](https://github.com/0xMiden/compiler).

`cargo miden` is a `cargo` cargo extension. Check out its [documentation](https://0xMiden.github.io/compiler/usage/cargo-miden/#compiling-to-miden-assembly)
for more details on how to build and run the compiled programs.

## Compile

```bash
cargo miden build --release
```

## Run

```bash
midenc run target/miden/release/collatz.masp --inputs inputs.toml
```

