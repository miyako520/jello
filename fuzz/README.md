# Fuzz targets

These targets use `cargo-fuzz` and LLVM `libFuzzer`. They are intentionally
separate from the normal test suite and require a nightly Rust toolchain.

Install the runner once, then run a bounded local session:

```text
cargo install cargo-fuzz
cargo fuzz run repair -- -max_total_time=300
cargo fuzz run diff -- -max_total_time=300
```

The repair target checks that successful JSON5 repairs remain strict JSON. The
diff target checks that arbitrary valid UTF-8 inputs do not panic. Inputs that
trigger a failure are written below `fuzz/artifacts/`; turn each minimized
failure into a regression test before deleting or replacing the artifact.
