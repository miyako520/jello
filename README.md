# jqr

`jqr` is a small Rust JSON formatter and repair-oriented parser experiment. It uses a handwritten lexer and recursive descent parser so diagnostics and repair behavior stay under project control.

## Quick Start

```powershell
'{"name":"Ada","items":[1,true,null]}' | cargo run --
```

In the MVP, pass a file path or pipe JSON through stdin:

```powershell
Get-Content data.json | cargo run --
cargo run -- --fix --stats --lang zh data.json
```

## Features

- Pretty-print standard JSON with two-space indentation.
- `--fix` repairs common mistakes before parsing:
  - single-quoted strings;
  - unquoted object keys;
  - trailing commas;
  - obvious missing commas between adjacent values.
- `--stats` prints key count, max depth, leaf count, size ratio, validity, and fix count to stderr.
- `--lang zh` and `--lang en` switch diagnostic text.

## Examples

English:

```powershell
cargo run -- --fix broken.json
```

中文：

```powershell
cargo run -- --fix --stats --lang zh broken.json
```

## Roadmap

- Add terminal color to diagnostics.
- Add more parser recovery paths.
- Add JSON5 input support.
- Add JSON Schema validation.
- Add GitHub Actions release builds for Linux, macOS, and Windows.
- Add Homebrew and `cargo install` publishing instructions once the package is published.

## Comparison

| Tool | Formatting | Repair | Handwritten diagnostics | Stats |
| --- | --- | --- | --- | --- |
| `jqr` | Yes | MVP rules | Yes | Yes |
| `jq` | Yes | No | No | No |
| `json_pp` | Yes | No | No | No |
