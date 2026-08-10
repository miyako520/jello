# Changelog

All notable changes to this project are documented in this file. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.1] - 2026-08-10

### Added

- Added a bounded line-level Myers diff engine to the core crate, `--diff` to
  the CLI and easy mode, and a live Changes panel to the desktop application.
- Added `--schema <path>` to validate canonical output against local JSON
  Schema Draft 2020-12 documents. The shared core validator confines relative
  references to the schema directory, blocks network references, caches
  compiled schemas, and enforces file, byte, and node budgets.
- Persisted the desktop language choice in the same
  `%LOCALAPPDATA%\Jello\config` file used by `jello-drop.exe`.
- Added a desktop preview Copy button and exposed invalid repair-review
  diagnostics in the Problems panel.
- Added property tests, targeted boundary tests, and `cargo-fuzz` repair and
  diff targets for arbitrary Unicode and structured JSON5 inputs.
- Added interactive repair review to the desktop application: accept or reject
  each repair group or all groups, with pending, accepted, and rejected ranges
  highlighted in the source pane; clicking a repair focuses its ranges and
  scrolls the editor to them. The preview pane marks ranges that still await a
  decision with a single subdued highlight.
- Added a public `save_updated` writer that atomically replaces a file after
  verifying its on-disk content still matches, so the desktop application can
  refresh its own output instead of stacking numbered files.
- Added session adoption of saved output files: consecutive saves update the
  same `.fixed` file in place, while the opened original stays protected.
- Cached syntax-highlighting tokenization and reduced overlay lookup to a
  single sweep so repaint frames do not re-tokenize or re-scan unchanged text.

### Changed

- Raised the core minimum supported Rust version from 1.73 to 1.85 and pinned
  the IDNA/ICU dependency chain to versions compatible with that toolchain.
- Release verification now checks all core features with the declared MSRV.
- Localized remaining desktop strings (save, accept/reject controls, invalid
  preview states) in both English and Chinese.
- The desktop application now caches the formatted preview between frames and
  refreshes it only when an analysis result is applied.

### Fixed

- Restored cooperative cancellation throughout shared Schema loading, cache
  validation, compilation boundaries, and issue collection so superseded GUI
  analyses are discarded promptly; bounded node counts limit work inside
  third-party calls that cannot be interrupted directly.
- Consecutive desktop saves no longer create `name.fixed-2.json`,
  `name.fixed-3.json` stacks; they update the session's output file in place.
- Desktop saves are refused when the target file changed externally after it
  was opened or written.
- Save As remains available when the session output file was removed or
  modified externally, so a preview can still be rescued to a new file.
- The status bar reports repairs as pending review instead of "waiting" when
  the current review is invalid.
- The status bar distinguishes an analysis in progress from idle waiting,
  including re-evaluations queued by repair decisions or schema changes.
- The status bar clears saved and error messages when the source is edited, so
  stale notices cannot cover up the current analysis or review state.
- Dropping several files no longer overwrites an open error with the "opened
  the first file" notice when the first file could not be opened.
- Preview highlights skip stale candidate ranges that fall outside the current
  repair plan instead of indexing past it.
- The drag-and-drop helper refuses symbolic links, matching the CLI and GUI.
- The preview pane marks only repairs that still await a decision with a
  single subdued highlight instead of repeating the source pane's three
  decision colors; accepted repairs are the final result and stay unmarked,
  and overlay backgrounds are lighter unless the repair is focused.
- Repair highlights now use theme-appropriate colors: the dark theme gets
  softer overlay backgrounds and brighter underlines instead of sharing the
  light theme palette.

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

[Unreleased]: https://github.com/miyako520/jello/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/miyako520/jello/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/miyako520/jello/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/miyako520/jello/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/miyako520/jello/releases/tag/v0.1.1
[0.1.0]: https://github.com/miyako520/jello/releases/tag/v0.1.0
