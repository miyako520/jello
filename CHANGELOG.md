# Changelog

All notable changes to this project are documented in this file. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Handwritten JSON lexer and recursive-descent parser with source spans.
- Two-space, configurable, and compact JSON formatting.
- Conservative structural repair with rule codes and source positions.
- A documented JSON5 input subset with strict JSON output.
- English and Chinese compiler-style diagnostics with ANSI color control.
- Structural statistics and a string-length histogram.
- `--check`, `--write`, `--indent`, `--compact`, and `--` CLI behavior.
- Opaque Rust `Document` API with parse, format, repair, and statistics entry
  points.
- Cross-platform CI, package smoke tests, and manual release rehearsal.

### Changed

- Renamed the unpublished package and executable from `jqr` to `jello`.
- Tightened strict mode to RFC 8259 whitespace, control-character, and UTF-16
  surrogate-pair behavior.
- Replaced global character-based repair passes with parser-context recovery.
- Renamed `RepairOutcome::Unchanged` to `RepairOutcome::Valid` so the variant
  describes repair status rather than byte-for-byte output equality.
- Preserved non-Unicode file paths and converted CLI output to fallible writes.
- Classified invalid UTF-8 and oversized input as invalid content rather than
  I/O failures.
- Corrected JSON5 keyword object keys and CR/CRLF line tracking.
- Completed Chinese argument, repair, check, and control-character messages.
- Bounded lexer diagnostics, tokens, repair edits, and formatted output.
- Made formatting fallible and separated strict structural repair from JSON5
  normalization.
- Added checked file replacement with Unix/Windows hard-link refusal, repeated
  concurrent-update checks, and pre-write statistics output.
- Corrected JSON5 whitespace, Unicode comment terminators and continuations,
  recovery after string escape errors, EOF source rendering, and CLI option
  precedence/color behavior.

[Unreleased]: https://github.com/miyako520/jello/commits/
