# jqr

`jqr` is a small Rust JSON formatter and repair-oriented parser experiment. It uses a handwritten lexer and recursive descent parser so diagnostics and repair behavior stay under project control.

## Quick Start

```powershell
'{"name":"Ada","items":[1,true,null]}' | cargo run --
```

Pass a file path or pipe JSON through stdin:

```powershell
Get-Content data.json | cargo run --
cargo run -- --fix --stats --lang zh data.json
cargo run -- --json5 --color always config.json5
```

## Features

- Pretty-print standard JSON with two-space indentation.
- `--fix` repairs common mistakes before parsing:
  - single-quoted strings;
  - unquoted object keys;
  - trailing commas;
  - obvious missing commas between adjacent values.
- `--stats` prints key count, max depth, leaf count, size ratio, validity, and fix count to stderr.
- `--stats` also renders a fixed-width histogram of object-key and string-value lengths.
- `--json5` accepts comments, single-quoted strings, unquoted keys, trailing commas, hexadecimal numbers, leading `+`, `.5`, `5.`, and string line continuations. Output is always standard JSON.
- `Infinity` and `NaN` are rejected because they cannot be converted to standard JSON without changing their meaning.
- `--lang zh` and `--lang en` switch diagnostic text.
- Diagnostics use Rust compiler-style source context and arrows.
- `--color auto|always|never` controls ANSI color. `NO_COLOR` always disables color.

## Examples

English:

```powershell
cargo run -- --fix broken.json
cargo run -- --json5 --stats config.json5
```

中文：

```powershell
cargo run -- --fix --stats --lang zh broken.json
cargo run -- --json5 --lang zh --color always config.json5
```

JSON5 input:

```json5
// config.json5
{
  appName: 'jqr',
  ports: [0x1f90, 8081,],
  ratio: +.5,
}
```

The command below prints strict, pretty-formatted JSON to stdout and statistics to stderr:

```powershell
cargo run -- --json5 --stats config.json5
```

## Roadmap

- Add more parser recovery paths.
- Add JSON Schema validation.
- Add GitHub Actions release builds for Linux, macOS, and Windows.
- Add Homebrew and `cargo install` publishing instructions once the package is published.

## Comparison

| Tool | Formatting | Repair | Handwritten diagnostics | Stats |
| --- | --- | --- | --- | --- |
| `jqr` | Yes | MVP rules | Yes | Yes |
| `jq` | Yes | No | No | No |
| `json_pp` | Yes | No | No | No |
