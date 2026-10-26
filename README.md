# fee-lens-rs

What a Solana transaction actually paid to land: base fee, priority fee and tips, separated, and
correct across legacy, v0 and v1 transactions. Point it at a transaction, a slot or an account.

This is the Rust implementation. A TypeScript one lives at
[solana-fee-lens](https://github.com/OsamaRaja92/solana-fee-lens). The two were written against the
same specification and agree: over a recorded mainnet block they reproduce all 574 charged fees to
the lamport and arrive at identical totals. `tests/block.rs` asserts the TypeScript figures
directly, so neither can drift unnoticed. The human-readable CLI output has matched byte for byte on
every transaction compared by hand, though that is not covered by a test; `--json` deliberately
differs, since field naming and number encoding follow each language's conventions.

```
$ fee-lens tx 5vasP9JBChatPSsd9Ybud6YXztPKf7FuCqMstb3EdQARfVf5ZmzCBAHoAT92V6W53aUCopka6XJPN9eBo2TfFGq1

Transaction 5vasP9JB..o2TfFGq1
  version            v1
  slot               461358125
  result             failed  {"InstructionError":[4,{"Custom":6001}]}

Fee
  base                        5,000 lamports  1 signature(s) x 5,000
  priority                    5,008 lamports  absolute total in the v1 header
  tips                            0 lamports  20,356 instructed, rolled back with the transaction
  charged                    10,008 lamports  0.000010008 SOL
  total to land              10,008 lamports  0.000010008 SOL, fee plus tips
  model check                 exact  the model reproduces the charged fee

Compute
  limit                     327,450 units  from the v1 header
  consumed                  121,199 units
  unused                    206,251 units  63.0% of the budget
  overpaid                        0 lamports  a v1 priority fee does not scale with the limit

ComputeBudget instructions present but inert
  This is a v1 transaction, so the runtime reads its limits from the header and ignores
  the 1 ComputeBudget instruction(s) in the message. Those instructions declare a
  limit of 0, against the header's 327,450. A tool that reads the instruction
  rather than the header reports the wrong figure here.

Tips
  recipient       service          lamports  via
  --------------  ---------------  --------  ---------
  4vieeG..pVN3Ey  not in registry    20,356  top-level
```

## The v1 problem

Since SIMD-0385, a v1 transaction carries its compute unit limit and priority fee in the message
header rather than in ComputeBudget instructions. Three consequences:

- **ComputeBudget instructions in a v1 transaction are inert.** The transaction above declares a
  limit of `0` while its header declares 327,450. Read the instruction and you report zero.
- **`priorityFee` is an absolute lamport total**, not the micro-lamports-per-unit rate that
  `SetComputeUnitPrice` sets. Treating one as the other is wrong by a factor of the limit over a
  million.
- **v1 is not an edge case.** It is 280 of 574 transactions in the recorded block, the largest of
  the three versions (v0 is 190, legacy 104).

Every RPC read passes `maxSupportedTransactionVersion: 1`. Omit it and the node refuses the entire
response with `-32015`; pass `0` and it refuses again, naming v1.

The example above is a failed transaction, which shows a second thing the tool gets right: its
instructions include a 20,356 lamport transfer, but a failed transaction is rolled back and only its
fee is taken, so nothing was paid. Counting instructed tips as paid overstates this block's tips as
67,345,751 lamports instead of 13,842,684.

The base fee has a second trap. It is 5,000 lamports per **signature**, and that includes signatures
verified by the Ed25519 and secp256 precompiles, not only the ones the message header requires.
Counting the header alone understates the fee by 5,000 lamports per precompile signature.
`fixtures/tx-ed25519-precompile.json` covers it.


## Verification

The model predicts each fee from the transaction's own directives. The metadata says what was
charged. The difference is reported on every analysis and should always be zero.

`crates/fee-lens-core/tests/block.rs` runs that over every transaction in a recorded mainnet block,
then checks every aggregate against the figures the TypeScript implementation produces for the same
fixture, so neither can drift unnoticed:

```
574 / 574 transactions reproduced exactly
```

Tests run against recorded JSON fixtures through the same code path as a live response. No network,
no RPC endpoint: clone and `cargo test`.

## Use

Rust 1.99.1, pinned in `rust-toolchain.toml` so a clone and CI lint against the same clippy.

```bash
cargo build --release

./target/release/fee-lens tx <signature>
./target/release/fee-lens slot 461358125 --top 10
./target/release/fee-lens account <address> --limit 50 --concurrency 1
./target/release/fee-lens slot 461358125 --json
```

RPC endpoint comes from `--rpc`, else `$SOLANA_RPC_URL`, else the public endpoint. Read calls only;
no key or signer. The public endpoint rate limits, so keep `--limit` small or pass `--concurrency 1`
for `account`, which makes one request per transaction.

`--tip-account ADDRESS=LABEL` names relays the registry does not know. `--known-tips-only`
restricts tips to registry matches.

## Layout

`fee-lens-core` holds the model and never sees an HTTP client; `wire.rs` is the single seam where
untyped JSON becomes typed data, and everything past it is pure. `fee-lens-cli` does networking and
rendering, using blocking HTTP and scoped threads rather than an async runtime: the work is a
handful of requests with bounded concurrency.

A compute unit price is a `u64` and its product with a limit overflows `u64`, so the charge is
computed in `u128` and saturates on the way back. Saturation is unreachable for any submittable
transaction, since the chain cannot charge more lamports than exist.

`unsafe_code` forbidden, clippy clean at `pedantic`.

## Limitations

- **Tip recipients mostly cannot be named.** The registry holds Jito's eight published accounts.
  Everything else is matched by shape: a top-level transfer from the fee payer to an account no
  other instruction touches, in a transaction that also does something besides move SOL. Among the
  transactions in the shipped block that succeeded, it finds 22 payments to 16 distinct recipients
  against 66 to the Jito registry — fewer payments, but 70% of the lamports (9,735,483 against
  4,107,201). It is a heuristic and is labelled as one.
- **A slot shows the transactions that landed.** Dropped ones are not in the block, so a percentile
  says what was sufficient, not what was required.
- **`TransferWithSeed` is not decoded.** Different layout, not used for tips in practice.
- **The derived-limit rule is checked against the builtins in the fixtures.** Others follow the
  documented rule but are unverified; an error shows up as a non-zero residual, not a wrong number.
- **Unknown transaction versions raise an error** rather than being priced by the wrong rules.

MIT.
