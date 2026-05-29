# Module Info: Full Guide

This is the detailed reference for `module_info`. For a quick start, see the
[README](../README.md). For the API reference, see
[docs.rs/module-info](https://docs.rs/module-info).

## Contents

- [How it works](#how-it-works)
- [Limitations](#limitations)
- [Metadata fields](#metadata-fields)
- [Configuration](#configuration)
  - [Static values in `Cargo.toml`](#static-values-in-cargotoml)
  - [Environment variables (CI build numbers)](#environment-variables-ci-build-numbers)
  - [Preserving SemVer-style pre-release / build suffixes](#preserving-semver-style-pre-release--build-suffixes)
  - [Disabling optional fields](#disabling-optional-fields)
  - [Debug output](#debug-output)
  - [Full `Cargo.toml` example](#full-cargotoml-example)
- [Custom `build.rs`: supplying metadata programmatically](#custom-buildrs-supplying-metadata-programmatically)
- [CI integration](#ci-integration)
- [Cross-compilation](#cross-compilation)
- [Validation and inspection](#validation-and-inspection)
- [Unit tests](#unit-tests)
- [Examples](#examples)
- [Troubleshooting](#troubleshooting)
- [Error handling](#error-handling)
- [Security considerations](#security-considerations)
- [Git metadata implementation](#git-metadata-implementation)
- [Technical details](#technical-details)
- [References](#references)

## How it works

`module_info` does three things:

1. **Embeds metadata at build time.** Your `build.rs` collects values from
   `Cargo.toml`, git, environment variables, and the OS, then writes them into
   the ELF binary's `.note.package` section.
2. **Exposes metadata at runtime.** The `get_module_info!` macro reads the
   note section back from the running module.
3. **Preserves metadata in crash dumps.** Because the data lives in an ELF
   note, it travels with a core dump, so post-mortem tooling sees exactly which
   build crashed even if the binary is gone.

The output follows the [systemd package-metadata
spec](https://uapi-group.org/specifications/specs/package_metadata_for_executable_files/),
so existing crash-analysis tooling (`coredumpctl`, `readelf -n`, GDB scripts)
consumes it without changes.

> **Note on shared libraries.** `module-info` embeds `.note.package` into both
> executables and `cdylib`s. The runtime `get_module_info!` accessor reads the
> *current ELF module's* note: a `cdylib` loaded via `dlopen` reads its own
> metadata, but an `rlib` (or any code statically linked into the final
> executable) reads the *executable's* values, because the linker-script
> directive is not propagated from `rlib` deps to the consumer's final link.
> To read a statically linked library's own metadata, parse the note section
> from the file on disk.

## Limitations

- **Linux/ELF only.** On other targets the build-time entry points are no-ops
  and the runtime accessors return `NotAvailable`. The metadata is embedded
  only in ELF objects.
- **GNU ld / gold linker.** The section is placed with an `INSERT AFTER` linker
  script (see [Troubleshooting](#troubleshooting)). `lld` and `mold` may handle
  it differently; verify the section is present if you use them.
- **ASCII-only fields.** Values are sanitized to printable ASCII. `©`/`®`/`™`
  map to `(c)`/`(r)`/`(tm)`; all other non-ASCII (accents, curly quotes, CJK,
  em-dashes) is dropped, and `"`, `\`, and control characters are removed. See
  [Security considerations](#security-considerations).
- **1 KiB total.** The serialized JSON must fit in `MAX_JSON_SIZE` (1 KiB) or
  the build fails with `MetadataTooLarge`. Glyph expansion (`©` -> `(c)`) counts
  against the limit.
- **Build-script context.** `generate_project_metadata_and_linker_script()` and
  `embed_package_metadata()` must run inside a Cargo `build.rs`; they rely on
  `OUT_DIR` and `CARGO_*`. See [Build-script context](#build-script-context).
- **Minimum Rust 1.74.**

The section is allocated (the `A` flag in `readelf -S`), so it survives `strip`.

## Metadata fields

Available through the `get_module_info!` macro:

| Field           | Description           | Source                              | Note                                   |
|-----------------|-----------------------|-------------------------------------|----------------------------------------|
| `binary`        | Binary/library name   | `Cargo.toml` package name           |                                        |
| `moduleVersion` | Full module version   | Package or environment variable     | 4 numeric parts                        |
| `version`       | Crate version         | `Cargo.toml` version                | 3 numeric parts                        |
| `maintainer`    | Maintainer            | `package.metadata.module_info`      | Contact address or unique identifier   |
| `name`          | Package name          | `Cargo.toml` package name           |                                        |
| `type`          | Module type           | `package.metadata.module_info`      |                                        |
| `repo`          | Git repository name   | Detected from git                   |                                        |
| `branch`        | Git branch            | Detected from git                   |                                        |
| `hash`          | Git commit hash       | Detected from git                   |                                        |
| `copyright`     | Copyright             | `package.metadata.module_info`      |                                        |
| `os`            | OS name               | Detected at build time              |                                        |
| `osVersion`     | OS version            | Detected at build time              |                                        |

Seven keys are **required** and must be non-empty: `binary`, `version`,
`moduleVersion`, `name`, `maintainer`, `os`, `osVersion`. The rest (`type`,
`repo`, `branch`, `hash`, `copyright`) are optional.

Together they form the JSON record stored in `.note.package` (one key/value
pair per line, no spaces around the colon, ASCII-only):

```json
{
"binary":"sample_crashing_process",
"moduleVersion":"0.1.0.0",
"version":"0.1.0",
"maintainer":"team@contoso.com",
"name":"sample_crashing_process",
"type":"tool",
"repo":"module-info",
"branch":"main",
"hash":"9fbf13be41d9c29f056588f6ef97509e534a51f5",
"copyright":"Contoso, Ltd.",
"os":"ubuntu",
"osVersion":"20.04"
}
```

Every key is always present; optional fields you don't set ship as `""`. See
[Validation and inspection](#validation-and-inspection) for how to read this
back out of a built binary or a core dump.

## Configuration

### Static values in `Cargo.toml`

Only `maintainer`, `type`, `copyright`, `version_env_var_name`,
`module_version_env_var_name`, and `allow_prerelease_suffix` are read from
`[package.metadata.module_info]`.
`version` comes from the outer `[package]` `version` field; `os`, `osVersion`,
`repo`, `branch`, and `hash` are collected automatically from the build
environment (env vars, git, `/etc/os-release`).

`maintainer` can be a contact email address or a UUID identifying the owning
team in your directory; use whichever your support tooling expects.

```toml
[package.metadata.module_info]
maintainer = "team@contoso.com"           # or a UUID like "cafeface-c0de-feed-beef-feedf00dd0d0"
type = "agent"
copyright = "Contoso, Ltd."
```

`type` is a free-form label; common values are `agent`, `tool`, `util`,
`library`, and `executable`, but any string your tooling expects works.

### Environment variables (CI build numbers)

Point `version_env_var_name` and `module_version_env_var_name` at the variable
your pipeline exports; the crate reads it at build time and embeds it.

```toml
[package.metadata.module_info]
version_env_var_name = "BUILD_BUILDNUMBER"
module_version_env_var_name = "BUILD_BUILDNUMBER"
```

For example, Azure Pipelines exposes
[`Build.BuildNumber`](https://learn.microsoft.com/en-us/azure/devops/pipelines/build/variables?view=azure-devops&tabs=yaml)
as `BUILD_BUILDNUMBER`. See [CI integration](#ci-integration) for more.

### Preserving SemVer-style pre-release / build suffixes

By default, anything after the first `-` or `+` in the resolved version string
is stripped before formatting, so a CI build number like
`"7.5.3.0-PullRequest-12345"` embeds as `"7.5.3.0"`. Some pipelines want the
buddy / PR identifier in the binary for traceability. Opt in by setting
`allow_prerelease_suffix = true`:

```toml
[package.metadata.module_info]
version_env_var_name = "BUILD_BUILDNUMBER"
module_version_env_var_name = "BUILD_BUILDNUMBER"
allow_prerelease_suffix = true
```

With this flag set, the numeric core is still normalized to 3 (`version`) or 4
(`moduleVersion`) dot-separated u16 components and the same u16 range check runs
against the numeric core, but the suffix (`-PullRequest-12345`, `+build-42`,
etc.) is re-attached to the formatted result. The embedded values become:

- `version` -> `"7.5.3-PullRequest-12345"`
- `moduleVersion` -> `"7.5.3.0-PullRequest-12345"`

Downstream consumers that parse `moduleVersion` as a strict 4-`WORD`
[`VS_FIXEDFILEINFO`](https://learn.microsoft.com/en-us/windows/win32/api/verrsrc/ns-verrsrc-vs_fixedfileinfo)
shape will see only the numeric core; suffix-aware tooling reads the full
string. Keep the default (`false`) unless your consumer chain understands the
suffix.

### Disabling optional fields

Not every crate wants to ship a repo name, branch, commit hash, or module type.
`type`, `repo`, `branch`, `hash`, and `copyright` are optional. The fields that
must be present and non-empty are the seven identity-plus-platform keys:
`binary`, `version`, `moduleVersion`, `name`, `maintainer`, `os`, `osVersion`.
(`os`/`osVersion` are auto-detected from `/etc/os-release` by
`PackageMetadata::from_cargo_toml`, so most builders don't supply them.)

The `.note.package` layout is fixed, so every key still appears in the JSON;
disabled fields ship as empty strings (`""`), which downstream tooling can skip.

- **Opt out via `Cargo.toml`:** omit `type`, `copyright`, and the
  `*_env_var_name` keys. `repo`, `branch`, and `hash` are git-derived and fall
  back to `"unknown"` if git isn't available; to force them off even inside a
  git checkout, use the builder API below.

- **Opt out via `build.rs` (builder API):** clear the fields you don't want
  after `from_cargo_toml()`:

    ```rust,no_run
    // build.rs: keep identity fields, drop git and module type.
    use module_info::{embed_package_metadata, EmbedOptions, PackageMetadata};

    fn main() -> Result<(), Box<dyn std::error::Error>> {
        let mut md = PackageMetadata::from_cargo_toml()?;
        md.repo.clear();
        md.branch.clear();
        md.hash.clear();
        md.module_type.clear();
        // binary, version, module_version, name, and maintainer must remain
        // non-empty; the build fails otherwise.
        embed_package_metadata(&md, &EmbedOptions::default())?;
        Ok(())
    }
    ```

If any of the seven required fields ends up empty, `validate_embedded_json`
fails the build with `MalformedJson(...)` naming the offending key.

### Debug output

`MODULE_INFO_DEBUG=true` enables detailed debug messages during build and at
runtime; unset (or `false`) is quiet. Useful for diagnosing metadata
generation or retrieval.

```bash
MODULE_INFO_DEBUG=true cargo build
```

### Full `Cargo.toml` example

```toml
[package]
name = "sample_crashing_process"
version = "0.1.0"
edition = "2021"
build = "build.rs"
authors = ["Team Name <example@contoso.com>"]

[package.metadata.module_info]
maintainer = "contact@contoso.com"
type = "tool"
copyright = "Contoso, Ltd."
version_env_var_name = "BUILD_BUILDNUMBER"
module_version_env_var_name = "BUILD_BUILDNUMBER"

[[bin]]
name = "sample_crashing_process"
path = "src/main.rs"

[dependencies]
module-info = { version = "0.5", features = ["embed-module-info"] }

[build-dependencies]
module-info = { version = "0.5" }
```

## Custom `build.rs`: supplying metadata programmatically

The zero-config `generate_project_metadata_and_linker_script()` reads metadata
from `Cargo.toml`, env vars, git, and the OS, then emits the
`cargo:rustc-link-arg=-T<linker_script.ld>` directive so cargo passes the
script to the final link step.

Two flows don't fit that default, and the builder API exists for them:

1. **Supply metadata from `build.rs` without editing `Cargo.toml`**: hand a
   struct literal to `module_info::new`, or populate a `PackageMetadata`.
2. **Static-library flows where the final link happens in a later build step**:
   write the linker script to a known directory and suppress the
   `cargo:rustc-link-arg` directive so you can pass the script to the outer
   linker yourself.

### Option A: one-call struct literal via `module_info::new(Info { … })`

Terser, and the field names match the embedded JSON shape (`r#type`,
`moduleVersion`, `osVersion`). `Info` is intentionally **not**
`#[non_exhaustive]`, so you can build the full note artifacts in one struct
literal. Always end with `..Default::default()` so new fields in future minor
releases don't break your build script.

```rust
// In build.rs
fn main() -> Result<(), Box<dyn std::error::Error>> {
    module_info::new(module_info::Info {
        binary: "sample_info_api".into(),
        name: "sample_info_api".into(),
        maintainer: "info-api-demo@contoso.com".into(),
        r#type: "tool".into(),
        version: "3.1.4".into(),
        moduleVersion: "3.1.4.159".into(),
        os: "linux".into(),
        osVersion: "unknown".into(),
        // repo, branch, hash, copyright fall back to "". They are optional
        // and ship as empty strings in the embedded JSON.
        ..Default::default()
    })?;
    Ok(())
}
```

Internally `new` converts `Info` to `PackageMetadata` and calls
`embed_package_metadata` with `EmbedOptions::default()`. Reach for Option B
when you need to override `EmbedOptions` (custom `out_dir`, suppressed
`cargo:rustc-link-arg`, …).

> **No auto-detection on this path.** Whatever you put in the `Info` literal
> ships verbatim. `os`/`osVersion` are **not** read from `/etc/os-release`,
> and `repo`/`branch`/`hash` are **not** read from git. If you want
> auto-detection, use Option B starting from `PackageMetadata::from_cargo_toml()`.
> The seven required keys must all be non-empty or the build fails.

See `examples/sample_info_api/` for a runnable version.

### Option B: `PackageMetadata` + `EmbedOptions`

Use this when you want to start from the `Cargo.toml`-driven defaults and
selectively override, or when you need to customize `EmbedOptions`.
`PackageMetadata` and `EmbedOptions` are both `#[non_exhaustive]`; construct via
`Default::default()` and assign fields.

```rust
// In build.rs
use module_info::{embed_package_metadata, EmbedOptions, PackageMetadata};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Start from Cargo.toml defaults, override only what you need.
    let mut md = PackageMetadata::from_cargo_toml()?;
    md.maintainer = std::env::var("TEAM_MAINTAINER").unwrap_or_else(|_| md.maintainer);
    md.module_type = "agent".into();
    md.version = std::env::var("BUILD_VERSION").unwrap_or_else(|_| md.version);
    md.module_version = std::env::var("BUILD_MODULE_VERSION").unwrap_or_else(|_| md.module_version);

    // Static-library flow: write the linker script where the outer build
    // system can find it, and skip the cargo rustc-link-arg directive so the
    // outer build step passes linker_script.ld to its own linker.
    let mut opts = EmbedOptions::default();
    opts.out_dir = Some(std::env::temp_dir().join("module_info_out"));
    opts.emit_cargo_link_arg = false;

    let artifacts = embed_package_metadata(&md, &opts)?;
    println!(
        "cargo:warning=linker script written to {}",
        artifacts.linker_script_path.display()
    );
    Ok(())
}
```

See `examples/sample_builder_api/` for a runnable version.

If you just need the defaults, keep calling
`generate_project_metadata_and_linker_script()`. It is a thin wrapper over
`embed_package_metadata(&PackageMetadata::from_cargo_toml()?, &EmbedOptions::default())`.

### Constructing `PackageMetadata` entirely by hand

When you don't have a `Cargo.toml` to read (e.g. metadata comes from an
external manifest), build the struct from scratch. Start from
`Default::default()` so new fields in future minor releases don't break you:

```rust,no_run
use module_info::PackageMetadata;

let mut md = PackageMetadata::default();
md.binary = "my_tool".into();
md.name = "my_tool".into();
md.version = "1.2.3".into();
md.module_version = "1.2.3.4".into();
md.maintainer = "team@contoso.com".into();
md.module_type = "agent".into();
md.hash = "0000000000000000000000000000000000000000".into();
```

## CI integration

CI pipelines typically supply build numbers via environment variables. Point
`version_env_var_name` and `module_version_env_var_name` at the variable your
pipeline exports; the crate reads it at build time and embeds it.

```toml
[package.metadata.module_info]
version_env_var_name = "BUILD_BUILDNUMBER"        # Azure DevOps Pipelines
module_version_env_var_name = "BUILD_BUILDNUMBER" # or GITHUB_RUN_NUMBER, CI_PIPELINE_IID, etc.
```

For [Azure DevOps Pipelines](https://learn.microsoft.com/en-us/azure/devops/pipelines/build/variables),
`Build.BuildNumber` is exposed as `BUILD_BUILDNUMBER`. GitHub Actions exposes
`GITHUB_RUN_NUMBER`, GitLab uses `CI_PIPELINE_IID`. If the variable is unset,
the crate falls back to `Cargo.toml`'s `package.version`.

These variables differ in shape. `GITHUB_RUN_NUMBER` and `CI_PIPELINE_IID` are
bare incrementing integers (e.g. `42`), so they embed as `42.0.0` / `42.0.0.0`
after padding, which is fine for `moduleVersion` (a build identifier) but is not
a meaningful `version`. Azure's `BUILD_BUILDNUMBER` can be configured to a
dotted version. Point each key at a variable whose shape matches what you want
embedded.

The embedded `moduleVersion` is intentionally separate from `Cargo.toml`'s
`[package].version`. The latter is the SemVer string crates.io and `cargo` use
for dependency resolution; `moduleVersion` is the 4-part build identifier (e.g.
`5.2.100.0`) that the pipeline assigns and crash-triage tools key on. They can
be identical, but in pipeline builds they normally diverge.

### Local reproduction

Reproduce what the pipeline does by exporting the same variable. By default the
crate strips SemVer-style `-<prerelease>` and `+<buildmeta>` suffixes before
splitting on `.`, so pipeline-style build numbers normalize cleanly (to keep the
suffix, see [Preserving SemVer-style pre-release / build
suffixes](#preserving-semver-style-pre-release--build-suffixes)):

```bash
# Plain dotted numeric: passes through unchanged.
BUILD_BUILDNUMBER="5.2.100.0" cargo build
# embeds:  version: 5.2.100      moduleVersion: 5.2.100.0

# Azure DevOps PR build: SemVer pre-release suffix is stripped.
BUILD_BUILDNUMBER="5.2.100.0-PullRequest-123456" cargo build
# embeds:  version: 5.2.100      moduleVersion: 5.2.100.0

# SemVer pre-release label: stripped at the first `-`.
BUILD_BUILDNUMBER="2.10.0-beta.3" cargo build
# embeds:  version: 2.10.0       moduleVersion: 2.10.0.0

# SemVer build metadata: stripped at the first `+`.
BUILD_BUILDNUMBER="3.1.4+ci.42" cargo build
# embeds:  version: 3.1.4        moduleVersion: 3.1.4.0
```

Each dotted component must fit in a `u16` (0..=65535); out-of-range values fail
the build rather than silently wrap. A build number with fewer than four
numeric parts (`1.2.3`) is zero-padded on the right (`1.2.3.0`).

## Cross-compilation

`module_info` supports cross-compilation to various targets, including ARM64
Linux. Add the appropriate configuration to your `.cargo/config.toml`:

```toml
# ARM64 Linux target configuration
[target.aarch64-unknown-linux-gnu]
linker = "aarch64-linux-gnu-gcc"
ar = "aarch64-linux-gnu-ar"
```

Then:

```sh
cargo build --target aarch64-unknown-linux-gnu
```

Install the matching toolchain first (`sudo apt install gcc-aarch64-linux-gnu`
on Debian-derivatives), and set `PKG_CONFIG_ALLOW_CROSS=1` if any dependencies
use `pkg-config`.

## Validation and inspection

### Verify the note section exists

```sh
$ readelf -S ./sample_crashing_process
...
  [ 3] .note.package     NOTE             0000000000000390  00000390
       0000000000000154  0000000000000000   A       0     0     4
```

Make sure `.note.package` is type `NOTE`, not `PROGBITS`.

### View embedded metadata

`readelf -p .note.package` prints the section as printable strings:

```sh
$ readelf -p .note.package ./sample_crashing_process
```

For a clean dump without `readelf`'s caret-encoded newlines:

```sh
$ objcopy --dump-section .note.package=/dev/stdout ./sample_crashing_process
```

### Read a single field from a built binary

Package builders and CI scripts often need just one field. `objcopy
--dump-section` writes the raw payload to stdout and `jq` reads the JSON
directly. This works on every binutils version the crate supports:

```sh
$ objcopy --dump-section .note.package=/dev/stdout target/debug/sample_crashing_process \
    | tr -d '\0\n' \
    | grep -oE '\{.*\}' \
    | jq -r .moduleVersion
0.1.0.0
```

Or without `jq`. The crate emits one `key:value` per line, so a line-oriented
`grep` matches a single field directly:

```sh
$ objcopy --dump-section .note.package=/dev/stdout target/debug/sample_crashing_process \
    | grep -oE '"moduleVersion":"[^"]+"' \
    | cut -d'"' -f4
0.1.0.0
```

Binutils >= 2.39 decodes the FDO Packaging Metadata note natively, so `readelf
-n` alone prints the JSON on a `Packaging Metadata:` line. Older `readelf -p`
(binutils 2.34 ships with Ubuntu 20.04) truncates the printable-string dump
once the payload exceeds an internal buffer, silently dropping the last field
(often `osVersion`). Prefer `objcopy --dump-section` for portability; reach for
`readelf -n` only when you know the host has 2.39+.

### Examine the build-time JSON

```sh
$ cat target/debug/build/{package-name}-{hash}/out/module_info.json
```

### Hexdump the raw note bytes

Read the section's file offset and size from `readelf -WS` and feed them to
`hexdump` (hardcoded offsets won't work, because the address differs per binary):

```sh
$ BIN=target/debug/sample_crashing_process
$ read OFF SIZE < <(readelf -WS "$BIN" \
    | awk '/\.note\.package/ { printf "0x%s 0x%s\n", $5, $7 }')
$ hexdump -C -s "$OFF" -n "$SIZE" "$BIN"
```

### Reading from a core dump

Embedding the note is worthwhile because it survives in a core dump. The note
is placed in the first read-only page (see [First-page
placement](#first-page-placement)), so it is captured as long as the kernel's
`coredump_filter` dumps file-backed first pages, which the default does via its
ELF-headers bit.

Enable core dumps first:

```sh
$ ulimit -c unlimited
# Also make sure /proc/sys/kernel/core_pattern routes dumps somewhere readable.
```

A raw core file is an ELF file **without section headers**, so `readelf -n
core` reads the process notes (`NT_PRSTATUS`, ...), not `.note.package`. The
embedded JSON is still present as bytes in the dumped first page; extract a
field with `strings`:

```sh
$ strings core.dump | grep -oE '"moduleVersion":"[^"]+"' | cut -d'"' -f4
0.1.0.0
```

On a systemd host, `systemd-coredump` also records the packaging metadata in the
journal as `COREDUMP_PACKAGE_METADATA`. Use `coredumpctl dump <match> --output
core.dump` to write the core out for the `strings` step above; depending on the
systemd version, `coredumpctl info` and `journalctl` may also display the stored
metadata directly.

The `sample_crashing_process` example crashes on demand so you can walk through
this end to end.

## Unit tests

Test that your embedded metadata is correct:

```rust
#[cfg(test)]
mod tests {
    use module_info::get_module_info;

    #[test]
    fn test_metadata() -> Result<(), Box<dyn std::error::Error>> {
        // version is 3 parts (major.minor.patch); moduleVersion is 4 parts.
        assert_eq!(get_module_info!(ModuleInfoField::Binary)?, "your_package_name");
        assert_eq!(get_module_info!(ModuleInfoField::Version)?, "1.2.3");
        assert_eq!(get_module_info!(ModuleInfoField::ModuleVersion)?, "1.2.3.4");
        assert_eq!(get_module_info!(ModuleInfoField::Maintainer)?, "team@contoso.com");
        assert_eq!(get_module_info!(ModuleInfoField::Type)?, "agent");
        Ok(())
    }
}
```

`ModuleInfoField` does not need importing: the `get_module_info!` macro matches
the variant name as a token.

Run the crate's own tests with:

```sh
cargo test -p module-info --lib
```

## Examples

The crates under `examples/` are **standalone Cargo packages**, not `cargo`
examples. Each has its own `Cargo.toml` and `build.rs`; build them from their
own directories.

1. **`sample_lib`**: `cdylib` shared library that embeds `.note.package` and
   exposes `extern "C"` accessors so a `dlopen`-based loader can read the
   library's own metadata at runtime.
2. **`sample_elf_bin`**: ELF binary showing how to embed metadata and access
   it via `get_module_info!`, `get_version()`, and `get_module_version()`.
3. **`sample_elf_bin_with_lib`**: ELF binary that loads `sample_lib`'s `.so`
   via `dlopen` and reads both the executable's and the library's own
   `.note.package` data.
4. **`sample_crashing_process`**: tool that deliberately crashes with various
   signals so you can verify the note survives in the core dump.
5. **`sample_builder_api`**: `build.rs` using the explicit builder API
   (`PackageMetadata` + `embed_package_metadata`).
6. **`sample_info_api`**: `build.rs` using the one-call `module_info::new(Info
   { ... })` entry point, demonstrating the disable-fields pattern.

Build and run (from the crate root):

```bash
$ cargo build --manifest-path examples/sample_elf_bin/Cargo.toml
$ ./examples/sample_elf_bin/target/debug/sample_elf_bin
$ readelf -n ./examples/sample_elf_bin/target/debug/sample_elf_bin
```

`sample_elf_bin_with_lib` loads `libsample_lib.so` via `dlopen`, so build
`sample_lib` first:

```bash
$ cargo build --manifest-path examples/sample_lib/Cargo.toml
$ cargo build --manifest-path examples/sample_elf_bin_with_lib/Cargo.toml
$ ./examples/sample_elf_bin_with_lib/target/debug/sample_elf_bin_with_lib
```

The standalone-package layout sidesteps the linker-script ordering issues that
arise with cargo `[[example]]` entries. The `examples/` directory ships in the
GitHub repository but is excluded from the published crate, so clone the repo to
run them. If you prefer not to clone, copy any example's `Cargo.toml`,
`build.rs`, and `src/` into a new crate, which is the same shape a real consumer
uses.

## Troubleshooting

| Symptom | Likely cause | Fix |
|---------|--------------|-----|
| No `.note.package` section at all | The build script didn't run, or the linker dropped the section. | Confirm the `[build-dependencies]` entry and `build.rs` exist, call `module_info::embed!()` at the crate root, and enable the `embed-module-info` feature. |
| Section missing only with `lld`/`mold` | Alternative linkers may not honor the `INSERT AFTER` linker script. | Build with GNU `ld`/`gold`, or verify the section landed and report the linker. |
| Section present but typed `PROGBITS`, not `NOTE` | The linker emitted the bytes without the note type. | Check you are on a supported linker; inspect with `readelf -S`. |
| `get_module_info!` returns `NotAvailable` on Linux | The `embed-module-info` feature is off. | Enable it: `features = ["embed-module-info"]`. |
| Build fails with `MetadataTooLarge` | The JSON exceeds 1 KiB (glyph expansion like `©` -> `(c)` counts). | Shorten fields or drop optional ones. |
| Build fails: `moduleVersion` part out of range / wrong part count | A part exceeds `u16` (65535), or there are not 4 numeric parts. | Use four dot-separated parts, each 0-65535; suffixes after `-`/`+` are stripped. |
| A field is shorter than expected (e.g. `José` -> `Jos`) | Non-ASCII characters were stripped during sanitization. | Use an ASCII spelling; see [Limitations](#limitations). |
| Embedded git hash looks stale | The build was not re-run. | The build script watches `.git/HEAD`, `.git/refs`, and `.git/packed-refs`, so a fresh commit normally retriggers it; run `cargo clean` if it did not. |

## Error handling

All fallible functions return `ModuleInfoResult<T>` (a type alias for
`Result<T, ModuleInfoError>`). Import the types directly:

```rust
use module_info::{get_module_info, ModuleInfoError, ModuleInfoResult};
```

### Error variants

| Variant | Description | When it occurs |
|---------|-------------|----------------|
| `NotAvailable(String)` | Module info unavailable | The `embed-module-info` feature is off, or running on a non-Linux platform |
| `NullPointer` | Null pointer encountered | Extracting module info from a null pointer |
| `Utf8Error(..)` | UTF-8 parse error | The binary's module info isn't valid UTF-8 |
| `MalformedJson(String)` | JSON format error | The metadata doesn't match the expected JSON, or build-time validation rejects a missing/empty required field or out-of-range `moduleVersion` |
| `MetadataTooLarge(String)` | Over the size limit | Build time: serialized JSON exceeds `MAX_JSON_SIZE` (1 KiB) |
| `IoError(..)` | File I/O error | Build time: generating the linker script or reading `Cargo.toml` |
| `Other(..)` | Catch-all | Any other error |

### Handling errors

`ModuleInfoError` is `#[non_exhaustive]`, so a wildcard `Err(e)` arm is required
(it also keeps you source-compatible when new variants are added):

```rust
use module_info::{get_module_info, ModuleInfoError};

fn print_module_version() {
    match get_module_info!(ModuleInfoField::Version) {
        Ok(version) => println!("Module version: {}", version),
        Err(ModuleInfoError::NotAvailable(msg)) => {
            eprintln!("Module info not available: {}", msg);
            eprintln!("Enable the 'embed-module-info' feature in your Cargo.toml");
        },
        Err(ModuleInfoError::NullPointer) => {
            eprintln!("Module version pointer is null");
        },
        Err(ModuleInfoError::MalformedJson(msg)) => {
            eprintln!("Module version has invalid format: {}", msg);
        },
        Err(e) => eprintln!("Failed to get module version: {}", e), // required wildcard
    }
}
```

### Cross-platform

On non-Linux targets the runtime accessors return `NotAvailable`; handle it
gracefully:

```rust
use module_info::{get_module_info, ModuleInfoError};

fn log_binary_info() {
    match get_module_info!(ModuleInfoField::Binary) {
        Ok(name) => log::info!("Running binary: {}", name),
        Err(ModuleInfoError::NotAvailable(_)) => {
            log::debug!("Module info not available on this platform"); // expected off-Linux
        },
        Err(e) => log::warn!("Failed to get binary name: {}", e),
    }
}
```

## Security considerations

- **Metadata validation.** All metadata is validated for size before embedding;
  a 1 KiB limit prevents excessive memory consumption and potential
  denial-of-service vectors. The seven required identity-plus-platform fields
  are checked to be present and non-empty (the rest may be empty when a
  consumer opts out; see [Disabling optional fields](#disabling-optional-fields)).
- **Input sanitization.** All field values are sanitized so the payload is pure
  printable ASCII. Control characters (`\n`, `\r`, `\t`, ...) and the
  JSON-significant `"` and `\` are removed. Non-ASCII characters are dropped,
  except `©`/`®`/`™`, which map to `(c)`/`(r)`/`(tm)`. This means accented names
  and other Unicode lose characters silently (`José` -> `Jos`); prefer an ASCII
  spelling at the source.
- **JSON validation.** The JSON structure is validated and required fields
  checked; malformed JSON is rejected with a descriptive error.
- **Resource limitation.** OS-release file reading has a 10 KiB size limit;
  file operations use proper error handling and cleanup.

Recommendations: keep metadata concise, and avoid including sensitive
information in metadata fields.

## Git metadata implementation

`module_info` retrieves git information using git CLI commands at build time:

- **Branch name**: `git rev-parse --abbrev-ref HEAD`
- **Commit hash**: `git rev-parse HEAD`
- **Repository name**: derived from `git remote get-url origin`: the last `/`-
  or `:`-separated path segment with a trailing `.git` stripped (so
  `git@github.com:user/repo.git` → `repo`). Falls back to the project directory
  name when no remote is configured or git is unavailable.

Requirements: git installed, code in a git repository. If git is unavailable or
a command fails, `branch` and `hash` fall back to `"unknown"` (lowercase), and
`repo` falls back to the project directory name (or `"unknown"` if that too is
unavailable). These are distinct from the `os`/`osVersion` fallbacks
(`"Linux"`/`"Unknown"`), which apply only when `/etc/os-release` is missing.

### Build-script context

`generate_project_metadata_and_linker_script()` and `embed_package_metadata()`
rely on Cargo-provided environment variables (`OUT_DIR`, `CARGO_MANIFEST_DIR`,
`CARGO_PKG_*`) that only exist inside a `build.rs` invocation. Calling them
elsewhere either errors with `OUT_DIR` missing or falls back to
`"Unknown"`-shaped metadata. Both entry points also emit
`cargo:rerun-if-changed=` / `cargo:rerun-if-env-changed=` directives. Among the
watched paths are `.git/HEAD`, `.git/refs`, and `.git/packed-refs`, plus any
configured `*_env_var_name` variables, so a new commit, branch switch, or
changed build number retriggers the build and refreshes the embedded metadata
rather than leaving it stale. These directives are no-ops outside a build-script
context.

## Technical details

### ELF note section format

The metadata is stored in `.note.package` with:

- Owner name: `FDO`
- Type: `0xcafe1a7e`
- Content: JSON-formatted metadata

### Memory alignment

All data is aligned to 4-byte boundaries for binary compatibility.

### First-page placement

The linker script inserts `.note.package` after `.note.gnu.build-id`, which the
linker places in the first page of the ELF image. This keeps the note visible
in minimal coredumps that capture only the first read-only page. Verify with
`readelf -l <bin>`; the note should appear in the first `LOAD` segment.

## References

- [ELF Format Specification](https://refspecs.linuxfoundation.org/elf/elf.pdf)
- [ELF Note Sections](https://docs.oracle.com/cd/E23824_01/html/819-0690/chapter6-18048.html)
- [Package metadata spec (UAPI Group)](https://uapi-group.org/specifications/specs/package_metadata_for_executable_files/)
- [Linux crash dumps with WinDbg](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/linux-crash-dumps)
- [Analyze crash dump files by using WinDbg](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/crash-dump-files)
- [Linux symbols and sources](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/linux-dwarf-symbols)
- [WinDbg Release notes](https://learn.microsoft.com/en-us/windows-hardware/drivers/debuggercmds/windbg-release-notes)
