# SWAPp Note

A Miden note script implementing a partially-fillable swap (PSWAP) for decentralized exchange functionality.

## How It Works

A creator locks an **offered asset** in the note and specifies a **requested asset** and amount. A consumer can fill the swap fully or partially by providing some or all of the requested asset.

- **Proportional exchange** — Output amounts are calculated proportionally to the input.
- **Partial fills** — If only partially filled, a remainder SWAPp note is automatically created with the leftover offered asset.
- **Surplus capture** — Solvers can earn spread in cross-swap scenarios via a surplus P2ID note.
- **Self-cancellation** — The note creator can consume their own note to reclaim assets.

## Note Inputs (set at creation)

| Index | Field |
|-------|-------|
| 0-3 | Requested asset (id prefix, id suffix, padding, total amount) |
| 4-5 | Creator account ID (prefix, suffix) |
| 6 | Note type |
| 7 | Tag |

## Note Args (provided by consumer)

| Index | Field |
|-------|-------|
| 0 | `input_amount` — amount of requested asset provided |
| 1 | `inflight_amount` — inflight requested asset amount |
| 2 | `surplus_amount` — offered asset surplus for solver |
| 3 | `consumer_p2id_tag` — P2ID tag for surplus note |

## Build

```sh
cargo miden build --manifest-path contracts/swapp-note/Cargo.toml --release
```
