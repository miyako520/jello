# Changelog

All notable changes to this project are documented in this file. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-07-15

### Added

- Handwritten JSON lexer and recursive descent parser with source spans.
- Two-space JSON formatter and repair rules for common malformed input.
- Lossless JSON5-compatible input mode for comments, single quotes, unquoted keys, trailing commas, and extended finite numbers.
- English and Chinese Rust compiler-style diagnostics with automatic or forced ANSI color.
- Structural statistics and a string-length histogram.
- File and stdin input with 16 MiB size and 256-level nesting limits.
- Cross-platform CI and tag-driven GitHub release automation.

[Unreleased]: https://github.com/miyako520/jello/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/miyako520/jello/releases/tag/v0.1.0
