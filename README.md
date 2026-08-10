# jello

`jello` is a small, handwritten JSON formatter, validator, and conservative
repair tool. It keeps parsing, diagnostics, and repair decisions under project
control instead of delegating them to a general-purpose JSON library.

Older unpublished revisions used the working name `jqr`.

## Installation

Install the latest source revision with Rust 1.85 or newer:

```powershell
cargo install --git https://github.com/miyako520/jello --locked --features schema
```

The project is not yet published to crates.io or Homebrew. Tagged releases
publish Linux, macOS, and Windows archives plus `SHA256SUMS` on the
[GitHub Releases page](https://github.com/miyako520/jello/releases).

## Quick start

For the simplest workflow, let Jello accept its supported JSON5 syntax,
conservatively repair the document, print the formatted JSON, and save a new
file without changing the source:

```powershell
jello easy data.json
```

This creates `data.fixed.json`. If that name already exists, Jello uses
`data.fixed-2.json`, then `data.fixed-3.json`, and so on. Unrepairable input
does not create an output file. The new file inherits the source file's
ordinary permissions.

### Windows desktop app

Windows release archives include `jello-gui.exe` for a visual, single-file
workflow. Open a JSON or JSON5 file, or drag one into the window. If several
files are dropped together, the GUI opens the first and directs batch work to
`jello-drop.exe`. The source remains editable on the left while strict,
formatted JSON updates on the right after a short pause. Problems, applied
repairs, and optional JSON Schema results appear in the collapsible panel below
the editors.

`Save .fixed` creates a new file beside the source, and `Save As` writes to a
new path you choose. Neither action overwrites an existing file. The toolbar
can load a local JSON Schema Draft 2020-12 document; relative references are
limited to files inside that schema's directory and network references are
blocked. The language selector switches between English and Chinese; the
choice is remembered in `%LOCALAPPDATA%\Jello\config`, the same file used by
`jello-drop.exe`.

The collapsible panel has four tabs: problems, repairs, schema, and changes.
The changes tab shows a live line diff between the source and the current
preview, updating as repairs are accepted or rejected. The preview header's
`Copy` button copies the formatted output to the clipboard.

### Windows drag and drop

Windows release archives also include `jello-drop.exe`. Drag one or more JSON
files onto it to repair and format every file in one step. Each successful
result is saved beside its source as a new `.fixed` file; source files are
never changed. The terminal reports progress and a final summary without
printing the full JSON documents.

On first use, choose English or Chinese. Jello remembers the choice in
`%LOCALAPPDATA%\Jello\config`. Double-click `jello-drop.exe` without dragging
files to change the language or view the instructions. A one-run language
override is also available:

```powershell
jello-drop.exe --lang zh data.json
```

A mixed batch continues after an individual failure. The exit code is `1` if
any file has a content error and `2` if any file has an I/O error.

Format stdin or a file:

```powershell
'{"name":"Ada","items":[1,true,null]}' | jello
jello data.json
```

Check formatting without printing JSON:

```powershell
jello --check data.json
```

Replace a checked regular file only after successful parsing and formatting:

```powershell
jello --write data.json
```

Repair supported mistakes and print the result:

```powershell
jello --fix broken.json
jello --fix --write broken.json
jello --fix --diff --json5 broken.json
jello --fix --schema schema.json data.json
```

## Command-line options

```text
jello [OPTIONS] [--] [path]
jello easy [OPTIONS] [--] <path>

easy                   Repair, print, and save as a new .fixed file

--fix                  Repair supported mistakes before formatting
--diff                 Print a unified diff of the fixed output instead of JSON
--stats                Print structural statistics to stderr
--check                Exit 1 when input is not already formatted
--write, -i            Replace a checked regular input file
--json5                Accept the documented JSON5 subset
--indent <0..16>       Pretty-print indentation width (default: 2)
--compact              Emit compact JSON
--lang <zh|en>         Diagnostic language
--schema <path>        Validate the output against a JSON Schema file
--color <MODE>         auto, always, or never
--version, -V          Print version
--help, -h             Print help
```

Use `--` before a path beginning with a hyphen. `--check` and `--write` are
mutually exclusive, as are `--compact` and `--indent`. `--diff` requires
`--fix` and cannot be combined with `--check` or `--write`.

Exit codes:

- `0`: success;
- `1`: invalid content or a failed formatting check;
- `2`: invalid arguments or an I/O failure.

### Diff and schema validation

`--fix --diff` prints a line-level unified diff instead of the repaired JSON.
It is also available in `easy` mode. When a file is too large or differs by
too many lines, Jello skips diff generation and reports that fact on stderr.

`--schema <path>` validates the canonical output against a local JSON Schema
Draft 2020-12 document. Schema violations exit with `1`; a schema load or
compile failure exits with `2`. Relative references are confined to the
schema's directory and network references are blocked. Released archives
include schema support; source installs need the `schema` feature shown above.

## Strict JSON behavior

Strict mode follows RFC 8259:

- only space, tab, carriage return, and line feed are accepted as whitespace;
- unescaped U+0000 through U+001F characters are rejected inside strings;
- valid UTF-16 surrogate pairs in `\uXXXX\uXXXX` escapes are decoded;
- lone surrogates and invalid JSON number forms are rejected.

Inputs are limited to 16 MiB, 250,000 tokens, and 256 nested arrays/objects.
At most 64 ordinary lexer diagnostics and 10,000 repair edits are retained.
Formatted output is limited to 64 MiB. Formatter output growth and the largest
lexer/parser buffers use fallible reservation; allocation failures in other
Rust or platform-library paths are not guaranteed to be recoverable.

## Repair safety

Without `--json5`, `--fix` only repairs structural mistakes: trailing commas
and missing commas between complete sibling values when actual whitespace or
JSON5 comment trivia separates them. JSON5 lexical syntax such as comments,
single-quoted strings, unquoted keys, hexadecimal numbers, and leading `+`
requires `--fix --json5`.

Repairs are decided from lexer tokens and parser container context, not global
text replacement. Every structural repair and JSON5 normalization is reported
to stderr with a rule code, byte offset, line, and column. Output is returned
only when the repaired document can be represented as strict JSON. `--fix`
never changes a file unless `--write` is also supplied.

`--write` refuses symbolic links and files with multiple hard links on Unix and
Windows. It snapshots file identity and metadata, rereads the content while
preparing output, and checks it again immediately before replacement. These
checks narrow concurrent-update races but are not an atomic compare-and-replace
operation. Replacement deliberately has directory-entry replacement semantics:
it uses a sibling temporary file and preserves ordinary permissions, but does
not preserve every platform ACL, extended attribute, alternate data stream,
owner, or timestamp.

Easy mode prints the formatted JSON before saving the new file. If saving
fails, the error explicitly says that terminal output was already produced.
Jello writes and synchronizes a hidden sibling temporary file first, then
atomically publishes it under an unused `.fixed` name. Existing output files
are never overwritten, and a write failure does not leave an incomplete
`.fixed` file. If the formatted file is published but the temporary hard link
cannot be removed, Jello reports a warning with the retained temporary path.

## Supported JSON5 subset

`--json5` accepts comments, single-quoted strings, unquoted identifier keys,
trailing commas, hexadecimal integers up to `u128`, leading `+`, `.5`, `5.`,
and string line continuations. Output is always strict JSON.

This is intentionally described as a subset. Full ECMAScript identifier and
escape syntax is not yet implemented. `Infinity` and `NaN` are rejected
because standard JSON cannot represent them without changing their meaning.

## Formatting and statistics

Pretty output uses two spaces by default. Use `--indent 0` through
`--indent 16`, or `--compact` for no insignificant whitespace.

`--stats` writes key count, maximum depth, leaf count, size ratio, validity,
repair count, and a string-length histogram to stderr so stdout remains
pipe-friendly.

## Rust library

The crate exposes an opaque `Document` so callers cannot construct invalid
JSON number strings:

```rust
use jello::{format, parse, FormatOptions};

let document = parse(r#"{"name":"Ada"}"#)?;
let output = format(&document, FormatOptions::compact()).expect("output is bounded");
assert_eq!(output, r#"{"name":"Ada"}"#);
# Ok::<(), Vec<jello::Diagnostic>>(())
```

The public API also provides `parse_json5`, `repair`, `repair_json5`,
`statistics`, `diff_lines`, and `unified_diff`. With the optional `schema`
feature, it additionally exposes `SchemaValidator` for local Draft 2020-12
validation. Formatting is fallible so callers can handle the output-size and
allocation limits.
`repair` returns `RepairOutcome::Valid` when no repair edits were needed;
its output is still canonical formatted JSON and may differ from the original
input.

## Development

```powershell
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features --locked
cargo package --locked
cargo fmt --manifest-path crates/jello-gui/Cargo.toml --all --check
cargo clippy --manifest-path crates/jello-gui/Cargo.toml --all-targets --locked -- -D warnings
cargo test --manifest-path crates/jello-gui/Cargo.toml --all-targets --locked
cargo build --manifest-path crates/jello-gui/Cargo.toml --release --locked
```

Property tests in `tests/properties.rs` run arbitrary Unicode and structured
JSON5 inputs through the parser, repair, plan, and diff APIs, and verify that
repair plans match the legacy repair path and that diffs round-trip. The
targeted limit tests in `tests/boundaries.rs` cover the nesting, token, and
multi-byte edit boundaries that random inputs rarely reach.

The `fuzz/` directory contains `cargo-fuzz` targets for the repair and diff
paths. They need a nightly toolchain and are not part of the normal test
suite:

```powershell
cargo install cargo-fuzz
cargo fuzz run repair -- -max_total_time=300
cargo fuzz run diff -- -max_total_time=300
```

CI runs the core checks on Linux, macOS, Windows, and Rust 1.85. Building the
desktop application from source requires Rust 1.92 because its `eframe` 0.35
dependency requires that toolchain; CI checks that minimum separately and
release-builds the GUI on Windows. Users of `jello-gui.exe` do not need Rust
installed.

## License

MIT

The Windows desktop application embeds Noto Sans CJK SC under the SIL Open
Font License 1.1. A copy of the font license is included with the Windows
release archive and in `crates/jello-gui/assets/NotoSansCJK-LICENSE.txt`.
