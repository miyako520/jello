# Changelog

All notable changes to this project are documented in this file. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-07-31

### Added

- Added a Windows desktop application with editable source and live formatted
  preview panes, syntax highlighting, clickable diagnostics, repair history,
  drag-and-drop input, light/dark themes, and English/Chinese UI switching.
- Added optional local JSON Schema Draft 2020-12 validation with instance and
  schema paths. Relative references are confined to the schema directory and
  network references are disabled.

### Changed

- Exposed stable UTF-8 input reading and the safe, non-overwriting `.fixed`
  output writer for reuse by the desktop application.
- Added compiled Schema caching with dependency-content invalidation and
  cooperative cancellation between analysis phases.
- Manual release rehearsals now use `-dev-<short-sha>` archive names so they
  cannot be mistaken for tagged release artifacts.
- Tagged releases rerun the desktop application's formatting, Clippy, and test
  gates on Windows before any GitHub Release can be published.

### Fixed

- Invalidated formatted previews immediately after source edits, preventing an
  older background result from being saved during the debounce interval.
- Limited Schema work to 64 local files and 32 MiB of aggregate Schema text.
- Bundled a Simplified Chinese fallback font so Chinese UI labels and JSON text
  render correctly instead of appearing as missing-glyph boxes.
- Clarified that the desktop editor opens one dropped file and directs batch
  processing to `jello-drop.exe`.

## [0.1.2] - 2026-07-31

### Added

- Added the optional Windows `jello-drop.exe` helper for processing one or
  more dragged JSON files with the same conservative JSON5 repair and safe
  non-overwriting `.fixed` output used by `jello easy`.
- Added a first-run English/Chinese chooser, persisted per-user language
  setting, one-run `--lang` override, no-file settings menu, per-file progress,
  mixed-batch continuation, and localized summaries.

### Changed

- Added a no-overwrite Windows move fallback when the filesystem does not
  support hard links, allowing `jello easy` and `jello-drop.exe` to publish
  `.fixed` files on locations such as exFAT volumes and compatible shares.

## [0.1.1] - 2026-07-30

### Added

- Added `jello easy <path>` for a beginner-friendly workflow that accepts the
  supported JSON5 subset, performs conservative repairs, prints the formatted
  result, and saves it to a non-overwriting `.fixed` sibling file while
  preserving ordinary source permissions.

## [0.1.0] - 2026-07-29

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

### Security

- Escaped untrusted terminal control characters in diagnostic messages, source
  labels, and source snippets while preserving accurate caret positions.

[Unreleased]: https://github.com/miyako520/jello/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/miyako520/jello/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/miyako520/jello/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/miyako520/jello/releases/tag/v0.1.1
[0.1.0]: https://github.com/miyako520/jello/releases/tag/v0.1.0
