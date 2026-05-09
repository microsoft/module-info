# Changelog

All notable changes to the `module-info` crate will be documented in this file.
(The crate is named `module-info` on crates.io; Rust import paths use
`module_info::…` since Cargo normalizes hyphens to underscores.)

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0] - 2026-04-21

First public release on crates.io.

**Platform support.** ELF `.note.package` embedding is Linux-only. On non-Linux
targets the macros and APIs expand to no-ops so the same source compiles
everywhere without `#[cfg]` guards at each call site.

**Minimum supported Rust version: 1.74.**

### Added

- Build-time embedding of ELF `.note.package` sections containing package
  metadata (binary, version, moduleVersion, maintainer, type, repo, branch,
  hash, copyright, os, osVersion) following the
  [systemd package-metadata spec](https://uapi-group.org/specifications/specs/package_metadata_for_executable_files/).
- Runtime access through `get_module_info!`, with a single-field form
  (`get_module_info!(ModuleInfoField::Binary)`) and a no-arg form returning a
  `HashMap<String, String>` of every embedded field.
- `get_version()` and `get_module_version()` accessors for the two most
  commonly read fields.
- `embed!()` macro: one-liner at the crate root that keeps the
  `.note.package` section in the final link. Expands to a `#[used] static` on
  Linux and to nothing elsewhere, so consumers don't need
  `#[allow(unused_imports)]`.
- Builder API for `build.rs` consumers that want to supply metadata
  programmatically or suppress the `cargo:rustc-link-arg` directive (for
  static-library flows): `PackageMetadata`, `PackageMetadata::from_cargo_toml()`,
  `embed_package_metadata(&md, &opts)`, `EmbedOptions`, and `EmbedArtifacts`.
- `Info` struct + `module_info::new(Info { … })` one-call convenience entry
  point. Field names match the embedded JSON shape (`r#type`, `moduleVersion`,
  `osVersion`).
- `moduleVersion` u16-range validation: rejects any value that isn't exactly
  four dot-separated numeric parts each fitting in a `u16`. Mirrors the
  Windows `VS_FIXEDFILEINFO::FILEVERSION` shape so crash-dump consumers parse
  every field without truncation.
- Required-field validation for `binary`, `version`, `moduleVersion`, `name`,
  `maintainer`, `os`, `osVersion`. `type`, `repo`, `branch`, `hash`,
  `copyright` are optional and may be left empty; the JSON shape stays fixed
  so consumers always see the same keys.
- Automatic collection of git (`branch`, `hash`, `repo`) and OS
  (`os`, `osVersion`) information, with `"unknown"` fallbacks when a source
  is unavailable.
- `ModuleInfoField` enum with `to_symbol_name()`, `to_key()`, and an `ALL`
  slice for iterating every supported field.
- `ModuleInfoError` / `ModuleInfoResult` with `#[non_exhaustive]` so new
  variants can land without a semver-major bump.

[Unreleased]: https://github.com/microsoft/module-info/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/microsoft/module-info/releases/tag/v0.5.0
