# Contributing

Thanks for helping improve `jello`.

## Development

Rust 1.73 is the minimum supported version. Before submitting a change, run:

```powershell
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets --locked
cargo package --locked
```

The lexer and parser are handwritten by design. Do not replace the core parse
path with `serde_json` or another JSON parser.

## Testing expectations

- Write a focused failing test before changing parser, formatter, repair, or
  CLI behavior.
- Lexer and parser changes require success and failure cases with source spans
  when relevant.
- Every repair rule requires a round-trip assertion proving the repaired output
  parses as strict JSON.
- User-facing CLI changes require an integration test in `tests/cli.rs`.
- JSON grammar changes require representative cases in `tests/conformance.rs`.

## Commit style

Use conventional commits where practical:

- `feat: add parser recovery for arrays`
- `fix: preserve escaped quotes in repair`
- `docs: update CLI examples`

## Release rehearsal

Run the GitHub Actions `Release` workflow manually before creating a tag. A
manual run builds and uploads every platform archive but does not create a
GitHub Release. A real tag must exactly match the Cargo version.
