# Contributing

Thanks for helping improve `jqr`.

## Development

```powershell
cargo fmt
cargo test
```

The lexer and parser are handwritten by design. Do not replace the core parse path with `serde_json` or another JSON parser.

## Commit Style

Use conventional commits where practical:

- `feat: add parser recovery for arrays`
- `fix: preserve escaped quotes in fixer`
- `docs: update CLI examples`

## Testing Expectations

Every repair rule should have a focused unit test. Parser and lexer changes should include success and failure cases with source spans when relevant.
