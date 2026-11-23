# Contributing

## Before opening a pull request

```
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --release

cd web
npm run wasm
npm run typecheck && npm run lint && npm test && npm run build
npm run e2e
```

`cargo test --workspace --release` includes a 1000-trial Monte Carlo, the
stacking sequences and the design comparison, and takes about five minutes.

## Ground rules

- **Numbers come from runs.** If a change can move a validation result,
  rerun `cargo run --release -p asterope-cli -- validate --trials 10000 --seed 1 --sweep --stress 300 --profiles 10000`
  and update `docs/validation.md` and `docs/validation-report.*` in the same
  pull request. Do not loosen a tolerance to make a test pass; report what
  was measured.
- **Native and WebAssembly must agree.** A change to the simulator, detector
  or solver changes the digest in `data/parity.txt`. Update it from the Rust
  test output and confirm `web/src/engine/parity.test.ts` passes with the new
  value.
- **No panics in the core.** `asterope-core`, `asterope-io` and `asterope-wasm`
  deny `unwrap`, `expect`, `panic!` and slice indexing. Return a `Result`.
  New entry points get a case in `tests/robustness.rs`.
- **Conventions are fixed.** Frames, quaternion order and signs are defined
  in `docs/conventions.md` and pinned by tests. A change there is a breaking
  change.
- **Comments explain why.** Cite the reference algorithm or state the frame
  and sign convention; do not narrate the code.
- Code, tests and documentation are in English.

## Reporting a frame that fails to solve

Attach the image if you can share it, the field of view you entered, and the
JSON export from the solution panel. A raw 16-bit frame can be reproduced
natively with `asterope solve` (see the README).
