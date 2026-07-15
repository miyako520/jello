# jqr

`jqr` is a small Rust JSON formatter and repair-oriented parser experiment. It uses a handwritten lexer and recursive descent parser so diagnostics and repair behavior stay under project control.

The executable and Rust package are named `jqr`; the source repository is hosted as [`miyako520/jello`](https://github.com/miyako520/jello).

## Installation

Install the latest source revision with Rust 1.73 or newer:

```powershell
cargo install --git https://github.com/miyako520/jello --locked
```

After a version is tagged, prebuilt Linux, macOS, and Windows archives and `SHA256SUMS` are published on the [GitHub Releases page](https://github.com/miyako520/jello/releases). Verify the archive checksum before placing `jqr` or `jqr.exe` on your `PATH`.

The project is not yet published to crates.io or Homebrew, so `cargo install jqr` and `brew install jqr` are not supported installation paths yet.

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

## Limits

- Inputs are limited to 16 MiB from files, stdin, and the parser library API.
- Arrays and objects may be nested up to 256 levels.
- JSON5 `Infinity` and `NaN` are rejected because standard JSON cannot represent them without changing their meaning.

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

## Releasing

The tag-driven release workflow requires the tag and Cargo package version to match. After CI succeeds on the intended commit, create and push a signed or annotated tag such as `v0.1.0`; GitHub Actions builds the platform archives, generates checksums, and creates the GitHub release. Tags containing a hyphen, such as `v0.2.0-alpha.1`, produce prereleases.

## Comparison

| Tool | Formatting | Repair | Handwritten diagnostics | Stats |
| --- | --- | --- | --- | --- |
| `jqr` | Yes | MVP rules | Yes | Yes |
| `jq` | Yes | No | No | No |
| `json_pp` | Yes | No | No | No |
