# Release Correctness Design

## Goal

Remove the known panic paths and correctness inconsistencies before the first
release while preserving the existing CLI contract: exit 0 for success, exit 1
for invalid content or check failures, and exit 2 for argument or I/O errors.

## CLI architecture

- Store the optional input path as `PathBuf` and parse process arguments with
  `env::args_os`. Known option names and option values remain Unicode, while a
  positional path may contain arbitrary platform-native bytes.
- Convert a path to lossy Unicode only when producing its diagnostic label.
  Temporary sibling paths retain the original `OsStr` filename.
- Distinguish content-read failures (`InvalidUtf8`, `InputTooLarge`) from I/O
  failures using a typed input error so their exit codes cannot be conflated.
- Route stdout and stderr through explicit `Write` operations. Every write
  returns `io::Result`; a failed write terminates with exit 2 rather than
  panicking.

## Parsing and positions

- In JSON5 object-key context, accept the `true`, `false`, and `null` tokens as
  the identifier names `"true"`, `"false"`, and `"null"`. Their value-context
  behavior remains unchanged.
- Treat CR, LF, and CRLF as line terminators for position tracking, counting a
  CRLF pair as one line break. JSON5 U+2028 and U+2029 are also line
  terminators because they are accepted as JSON5 whitespace.

## Repair API

Rename `RepairOutcome::Unchanged` to `RepairOutcome::Valid`. `Valid` means that
the input needed no repair edits; `RepairResult::output` remains canonical
formatted JSON and may differ byte-for-byte from the input.

## Localization

- Restore the damaged Chinese unescaped-control-character diagnostic.
- Render argument errors, repair edit descriptions, and check failures through
  language-aware message functions.
- Select a valid `--lang` value independently of argument order so an argument
  error can still be rendered in the requested language.
- Keep stable diagnostic and repair codes as the programmatic API; translation
  affects only human-facing CLI text.

## Verification

Add regression coverage for invalid UTF-8 and oversized input exit status,
non-Unicode Unix paths, failing output writers, all three JSON5 keyword keys,
CR/LF/CRLF positions, `RepairOutcome::Valid`, and Chinese CLI messages.
Formatting, checks, Clippy, tests, MSRV checks, documentation checks, and
packaging should be run when the required toolchain and linker are available.
