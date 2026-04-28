# Module Info

The `module_info` crate embeds build-time metadata (version, git commit,
maintainer, OS, etc.) from your Rust project into ELF binaries as
`.note.package` sections so **the metadata survives crashes**. When your
process dies and produces a core dump, the metadata travels with it.
`coredumpctl info`, `readelf -n core.dump`, GDB scripts, and any other
consumer of the [systemd package-metadata
format](https://uapi-group.org/specifications/specs/package_metadata_for_executable_files/) will see exactly
which build of code crashed, regardless of whether the binary is still on
disk or has been redeployed.

That is the crate's reason for existing. Runtime read-back via the
`get_module_info!` macro is a convenience for tooling that wants the
metadata while the process is still alive. Useful, but a bonus on top of
the main feature, not the point of the crate.

[![crates.io](https://img.shields.io/crates/v/module-info.svg)](https://crates.io/crates/module-info)
[![Documentation](https://docs.rs/module-info/badge.svg)](https://docs.rs/module-info)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## Platform Support

- **Linux:** full functionality. Emits a `.note.package` section into the
  binary at build time; runtime accessors read it back.
- **Windows/macOS/Other:** no-op embedding (nothing is emitted into the
  binary). Build-script entry points compile to empty stubs so the same
  source builds everywhere. Runtime accessors (`get_module_info!`,
  `extract_module_info`) return `ModuleInfoError::NotAvailable` rather
  than placeholder strings; handle that error in cross-platform code.

## Key Features

- Embeds the metadata directly into the ELF binary so it's recoverable from
  any core dump without external symbol files or build-system context.
- Follows the [systemd package-metadata spec](https://uapi-group.org/specifications/specs/package_metadata_for_executable_files/),
  so existing crash-analysis tooling consumes the output without changes.
- Captures git information (branch, commit hash, repo name), OS distro and
  version, and Cargo-package metadata (version, maintainer, copyright).
- Embeds metadata in both executables and shared libraries; the
  `.note.package` section is always present in either ELF type, so a core
  dump from either contains the metadata. (Note: the runtime
  `get_module_info!` accessor is reliable only from the main binary; a
  shared library reading its *own* metadata at runtime sees the executable's
  copy due to global symbol resolution. Parse the ELF note section directly
  if you need a library's own metadata at runtime.)
- Build-time only: zero runtime cost on the hot path; the runtime accessor
  is opt-in via the `embed-module-info` feature.

## Usage in Other Crates

### Embedding the Note Section

To embed the `.note.package` section in your binary or shared library, add a single macro invocation at the crate root (typically `src/main.rs` or `src/lib.rs`):

```rust
module_info::embed!();
```

This macro expands to a `#[used] static` that references the metadata symbols produced by the build script, forcing the linker to keep the `.note.package` section even when no runtime code calls `get_module_info!`. On non-Linux targets it expands to nothing, so the same source compiles everywhere.

If you also need the runtime API, import it explicitly:

```rust
module_info::embed!();
use module_info::get_module_info;
```

`ModuleInfoField::*` variants are matched as tokens by the `get_module_info!` macro, so you do not need to import the `ModuleInfoField` enum unless you reference it from your own code.

### High-Level Approach

The `module_info` crate provides three main functions:

1. **Embedding metadata at build time**: Information from your `Cargo.toml`, git repository, and environment variables is collected during the build process and embedded into the ELF binary's `.note.package` section.
2. **Retrieving metadata at runtime**: The `get_module_info!` macro allows you to access this metadata from within your running application.
3. **Preserving metadata in crash dumps**: When your application crashes, the ELF note section is preserved in the core dump, making it available for post-mortem analysis.

This approach ensures that version information, git commit hashes, and other vital metadata are always available, both during normal operation and when analyzing crash dumps.

## Integration

Add the following to your `Cargo.toml`:

### Add directly from crates.io

Add both the runtime and the build-script dependency:

```sh
cargo add module-info --features embed-module-info
cargo add --build module-info
```

…or edit `Cargo.toml` by hand:

```toml
[dependencies]
module-info = { version = "0.5", features = ["embed-module-info"] }

[build-dependencies]
module-info = { version = "0.5" }
```

## Quick Start

### Integration Steps

- Add the `"embed-module-info"` feature into your `Cargo.toml`
- Ensure `build.rs` is defined. Therefore, `module_info` crate will stamp your ELF binaries' note section.
- Optionally access metadata at runtime or in unit tests to validate your crate's or binary's content.
- Add unit tests

1. **Add a build script**

    Create a `build.rs` file in your project root:

    ```rust
    fn main() -> Result<(), Box<dyn std::error::Error>> {
        module_info::generate_project_metadata_and_linker_script()?;
        Ok(())
    }
    ```

2. **Configure metadata**

    Add metadata to your `Cargo.toml`:

    ```toml
    [package.metadata.module_info]
    maintainer = "example@contoso.com"
    copyright = "Contoso, Ltd."
    # Optional: specify module type (agent, tool, util, library, executable, etc.)
    type = "agent"
    ```

3. **Access metadata at runtime**

    ```rust
    module_info::embed!();
    use module_info::get_module_info;

    fn main() {
        // Get specific metadata fields
        if let Ok(binary) = get_module_info!(ModuleInfoField::Binary) {
            println!("Binary name: {}", binary);
        }

        if let Ok(version) = get_module_info!(ModuleInfoField::Version) {
            println!("Version: {}", version);
        }

        // Or get all metadata as a HashMap (returns ModuleInfoResult<HashMap<String, String>>)
        if let Ok(all_info) = get_module_info!() {
            for (key, value) in all_info {
                println!("{}: {}", key, value);
            }
        }
    }
    ```

## Custom build.rs: Supplying Metadata Programmatically

The zero-config `generate_project_metadata_and_linker_script()` reads metadata
from `Cargo.toml`, env vars, git, and the OS, then emits the
`cargo:rustc-link-arg=-T<linker_script.ld>` directive so cargo passes the
script to the final link step.

Two flows don't fit that default. `module_info` exposes an explicit
builder API to support them:

1. **Supply metadata from `build.rs` without editing `Cargo.toml`**: hand a
   struct literal to `module_info::new`, or populate a `PackageMetadata`
   programmatically.
2. **Static-library flows where the final link happens in a later build step**:
   write the linker script to a known directory and suppress the
   `cargo:rustc-link-arg` directive so you can pass the script to the outer
   linker explicitly.

### Option A: one-call struct literal via `module_info::new(Info { … })`

Terser, and the field names match the embedded JSON shape (`r#type`,
`moduleVersion`, `osVersion`). `Info` is intentionally **not**
`#[non_exhaustive]`, so you can build the full note artifacts in one struct
literal. Just always end with `..Default::default()` so new fields added in
future minor releases don't break your build script.

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
        // `repo`, `branch`, `hash`, `copyright` fall back to "". They are
        // optional and ship as empty strings in the embedded JSON.
        ..Default::default()
    })?;
    Ok(())
}
```

Internally `new` converts `Info` to `PackageMetadata` and calls
`embed_package_metadata` with `EmbedOptions::default()`. Reach for Option B
below when you need to override `EmbedOptions` (custom `out_dir`, suppressed
`cargo:rustc-link-arg`, …).

> **No auto-detection on this path.** Whatever you put in the `Info` literal
> ships verbatim. `os`/`osVersion` are **not** read from `/etc/os-release`,
> and `repo`/`branch`/`hash` are **not** read from git. You own every field.
> If you want `/etc/os-release` + git auto-detection, use Option B with
> `PackageMetadata::from_cargo_toml()` as the starting point. The seven
> required keys (`binary`, `version`, `moduleVersion`, `name`, `maintainer`,
> `os`, `osVersion`) must all be non-empty or `validate_embedded_json` will
> fail the build.

See `examples/sample_info_api/` for a runnable version of this flow.

### Option B: `PackageMetadata` + `EmbedOptions`

Use this shape when you want to start from the `Cargo.toml`-driven defaults
and selectively override, or when you need to customize `EmbedOptions`.
`PackageMetadata` and `EmbedOptions` are both `#[non_exhaustive]`, which
forbids struct-literal construction outside this crate; construct via
`Default::default()` and assign fields.

```rust
// In build.rs
use module_info::{embed_package_metadata, EmbedOptions, PackageMetadata};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Start from the Cargo.toml-driven defaults and override only the fields
    // you want to override programmatically.
    let mut md = PackageMetadata::from_cargo_toml()?;
    md.maintainer = std::env::var("TEAM_MAINTAINER").unwrap_or_else(|_| md.maintainer);
    md.module_type = "agent".into();
    md.version = std::env::var("BUILD_VERSION").unwrap_or_else(|_| md.version);
    md.module_version = std::env::var("BUILD_MODULE_VERSION").unwrap_or_else(|_| md.module_version);

    // Static-library flow: write the linker script to a directory the outer
    // build system knows about, and skip the cargo rustc-link-arg directive.
    // The outer build step will pass `linker_script.ld` to its own linker.
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

See `examples/sample_builder_api/` for a runnable version of this flow.

If you just need the defaults, keep calling
`generate_project_metadata_and_linker_script()`. It is a thin wrapper over
`embed_package_metadata(&PackageMetadata::from_cargo_toml()?, &EmbedOptions::default())`.

### Constructing `PackageMetadata` entirely by hand

When you do not have a `Cargo.toml` you want read (e.g. the metadata comes
from an external manifest), build the struct from scratch. `PackageMetadata`
is `#[non_exhaustive]`, so start from `Default::default()` and assign the
fields you need. That way new fields added in future minor releases do not
break your construction:

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

## Available Metadata Fields

The following metadata fields are available through the `get_module_info!` macro:

| Field           | Description                            | Source                               | Note                                 |
|-----------------|----------------------------------------|--------------------------------------|--------------------------------------|
| `binary`        | Binary/library name                    | `Cargo.toml` package name            |                                      |
| `moduleVersion` | Full module version                    | From package or environment variable | Consists of 4 numerical parts        |
| `version`       | Crate version                          | `Cargo.toml` version                 | Consists of 3 numerical parts        |
| `maintainer`    | Maintainer information                 | From `package.metadata.module_info`  | A contact address or unique identifier |
| `name`          | Package name                           | `Cargo.toml` package name            |                                      |
| `type`          | Module type                            | From `package.metadata.module_info`  |                                      |
| `repo`          | Git repository name                    | Detected from git                    |                                      |
| `branch`        | Git branch                             | Detected from git                    |                                      |
| `hash`          | Git commit hash                        | Detected from git                    |                                      |
| `copyright`     | Copyright information                  | From `package.metadata.module_info`  |                                      |
| `os`            | Operating system name                  | Detected at build time               |                                      |
| `osVersion`     | OS version                             | Detected at build time               |                                      |

## Configuration Options

### Using Static Values

Only `maintainer`, `type`, `copyright`, `version_env_var_name`, and
`module_version_env_var_name` are read from `[package.metadata.module_info]`.
`version` comes from the outer `[package]` `version` field, and `os`,
`osVersion`, `repo`, `branch`, `hash` are collected automatically from the
build environment (env vars, git, `/etc/os-release`).

`maintainer` can be either a contact email address or a UUID that identifies
the owning team in your directory; use whichever form your support tooling
expects.

```toml
[package.metadata.module_info]
maintainer = "team@contoso.com"           # or a UUID like "cafeface-c0de-feed-beef-feedf00dd0d0"
type = "agent"
copyright = "Contoso, Ltd."
```

### Using Environment Variables

Configure environment variable names to use for metadata values:

For example: For Azure Pipeline integration, [Build.BuildNumber](https://learn.microsoft.com/en-us/azure/devops/pipelines/build/variables?view=azure-devops&tabs=yaml) provides build version information, and it's represented `BUILD_BUILDNUMBER` as environment variable.

```toml
[package.metadata.module_info]
version_env_var_name = "BUILD_BUILDNUMBER"
module_version_env_var_name = "BUILD_BUILDNUMBER"
```

### Disabling fields

Not every crate wants to ship a repo name, branch, commit hash, or module
type in its binary. `module_info` treats `type`, `repo`, `branch`, `hash`,
and `copyright` as optional. The fields that must be present and non-empty
are the seven identity-plus-platform keys: `binary`, `version`,
`moduleVersion`, `name`, `maintainer`, `os`, and `osVersion`. (`os` and
`osVersion` are auto-detected from `/etc/os-release` by
`PackageMetadata::from_cargo_toml`, so most builders don't have to supply
them explicitly.)

The `.note.package` layout is fixed, so every key still appears in the
embedded JSON; disabled fields ship as empty strings (`""`), which
downstream tooling can skip. Concretely:

- **Opt out via `Cargo.toml`:** omit `type`, `copyright`, and the
  `*_env_var_name` keys from `[package.metadata.module_info]`. `repo`,
  `branch`, and `hash` are git-derived and fall back to `"unknown"` if git
  isn't available; to force them off even inside a git checkout, use the
  builder API below. (Do *not* try to disable `os`/`osVersion`; they're
  required, and `from_cargo_toml` populates them from `/etc/os-release`.)

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
        // `md.binary`, `md.version`, `md.module_version`, `md.name`, and
        // `md.maintainer` must remain non-empty; the build fails otherwise.
        embed_package_metadata(&md, &EmbedOptions::default())?;
        Ok(())
    }
    ```

    The same approach works with [`Info`] if you prefer the struct-literal
    convenience type: leave the field out of the literal and
    `..Default::default()` fills it with `""`.

If any of the seven required fields ends up empty, `validate_embedded_json`
fails the build with `MalformedJson(...)` naming the offending key. That
guardrail keeps someone from accidentally shipping a binary without a
usable identity, while still allowing an explicit opt-out of the optional
fields.

### Debug Output Control

The `MODULE_INFO_DEBUG` environment variable controls debug message output during build and at runtime:

```bash
# Enable debug output
MODULE_INFO_DEBUG=true cargo build

# Disable debug output (default)
MODULE_INFO_DEBUG=false cargo build
# or simply don't set the variable
cargo build
```

When enabled (value set to "true", case-insensitive), the `debug!` macro will output detailed information about the embedding process and other operations. This is useful for diagnosing issues with metadata generation or retrieval.

### Full Cargo.toml Example

```toml
[package]
name = "sample_crashing_process"
version = "0.1.2"
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

## Cross-Compilation Support

The `module_info` crate supports cross-compilation to various targets, including ARM64 Linux platforms. This section provides guidance on configuring cross-compilation settings and working with git repository information across different platforms.

### Cross-Compilation Configuration

To successfully cross-compile for different target architectures, add appropriate configuration to your `.cargo/config.toml` file.

Example configuration for common targets:

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
on Debian-derivatives), and set `PKG_CONFIG_ALLOW_CROSS=1` if any of your
dependencies use `pkg-config`.

## Validation and Inspection

### Validating Installation

Use `readelf` to verify the note section was properly added:

```sh
$ readelf -S ./sample_crashing_process
...
Section Headers:
  [Nr] Name              Type             Address           Offset
       Size              EntSize          Flags  Link  Info  Align
  [ 2] .note.gnu.build-i NOTE             000000000000036c  0000036c
       0000000000000024  0000000000000000   A       0     0     4
  [ 3] .note.package     NOTE             0000000000000390  00000390
       00000000000001a6  0000000000000000   A       0     0     8
  ...
```

Make sure `.note.package` is set as `NOTE`, not `PROGBITS`.

### Viewing Embedded Metadata

`readelf -p .note.package` prints the section as printable strings, so the
embedded JSON payload falls out directly without any field-count assumption:

```sh
$ readelf -p .note.package ./sample_crashing_process
```

For a clean dump without `readelf`'s caret-encoded newlines, pipe through
`objcopy`:

```sh
$ objcopy --dump-section .note.package=/dev/stdout ./sample_crashing_process
```

```sh
$ strings ./sample_crashing_process | grep -A 12 '"binary"'
"binary": "sample_crashing_process",
"moduleVersion": "0.1.0.0",
"version": "0.1.0",
"maintainer": "<GUID or maintainer contact email>",
"name": "sample_crashing_process",
"type": "agent",
"repo": "Project_Repository_Name",
"branch": "feature/addSomething",
"hash": "9fbf13be41d9c29f056588f6ef97509e534a51f5",
"copyright": "Microsoft",
"os": "ubuntu",
"osVersion": "20.04"
```

### Examining JSON Metadata

```bash
$ cat target/debug/build/{package-name}-{hash}/out/module_info.json
```

## Adding Unit Tests

Test that your embedded metadata is correct:

```rust
#[cfg(test)]
mod tests {
    use module_info::get_module_info;

    #[test]
    fn test_metadata() -> Result<(), Box<dyn std::error::Error>> {
        // Version is 3 parts (major.minor.patch); moduleVersion is 4 parts.
        assert_eq!(get_module_info!(ModuleInfoField::Binary)?, "your_package_name");
        assert_eq!(get_module_info!(ModuleInfoField::Version)?, "1.2.3");
        assert_eq!(get_module_info!(ModuleInfoField::ModuleVersion)?, "1.2.3.4");
        assert_eq!(get_module_info!(ModuleInfoField::Maintainer)?, "team@contoso.com");
        assert_eq!(get_module_info!(ModuleInfoField::Type)?, "agent");
        Ok(())
    }
}
```

This bundles project details directly into the compiled artifact so they can be inspected via external tools, by calling `get_module_info!` at runtime, or from a crash dump.

### Read a single field with `readelf -p`

Package builders and CI scripts often just need one field (e.g. `moduleVersion`) out of a built binary. `readelf -p .note.package` prints the section as printable strings, so the JSON payload falls out directly with no hex decoding and no extra tools:

```sh
$ readelf -p .note.package target/debug/sample_crashing_process

String dump of section '.note.package':
  [     0]  FDO
  [     8]  {"binary":"sample_crashing_process","version":"0.1.2","moduleVersion":"0.1.2.0", ...}
```

Extract a single field with `jq`:

```sh
$ readelf -p .note.package target/debug/sample_crashing_process \
    | grep -oE '\{.*\}' \
    | jq -r .moduleVersion
0.1.2.0
```

Or without `jq`, using plain shell:

```sh
$ readelf -p .note.package target/debug/sample_crashing_process \
    | grep -oE '"moduleVersion":"[^"]+"' \
    | cut -d'"' -f4
0.1.2.0
```

Binutils ≥ 2.39 also decodes the FDO Packaging Metadata note natively, so `readelf -n` alone prints the JSON on a `Packaging Metadata:` line. On older versions (e.g. Ubuntu 20.04 ships 2.34) `-n` only shows hex, so prefer `-p .note.package` for portability.

```json
$ cat target/debug/build/sample_crashing_process-<hash>/out/module_info.json
{
"binary": "sample_crashing_process",
"moduleVersion": "0.1.0.0",
"version": "0.1.0",
"maintainer": "<GUID or maintainer contact email>",
"name": "sample_crashing_process",
"type": "agent",
"repo": "Project_Repository_Name",
"branch": "feature/addSomething",
"hash": "ea43550c8868fe6ac0bd2b5b91970276d6586dc1",
"copyright": "Microsoft",
"os": "ubuntu",
"osVersion": "20.04"
}
```

To hexdump the raw note-section bytes for any binary, read the section's
file offset and size out of `readelf -WS` and feed them to `hexdump`:

```sh
$ BIN=target/debug/sample_crashing_process
$ read OFF SIZE < <(readelf -WS "$BIN" \
    | awk '/\.note\.package/ { printf "0x%s 0x%s\n", $5, $7 }')
$ hexdump -C -s "$OFF" -n "$SIZE" "$BIN"
```

Hardcoded offsets won't work; the `.note.package` address differs per
binary based on what other sections the linker placed ahead of it. The
`readelf`-driven version above is portable across any build.

Generated files for your build module can be found under `target/debug/build/<your_module_build_path>/out/` path.
Example path: `target/debug/build/sample_crashing_process-fd45dc7726bbc6d9/out/module_info.json`

## Run unit tests

`cargo test -p module-info --lib`

## Examples

The crates under `examples/` are **standalone Cargo packages**, not `cargo`
examples. Each has its own `Cargo.toml` and `build.rs`; build them from their
own directories:

1. `sample_lib`: shared library (cdylib+rlib) example showing how to:
   - Embed metadata into a shared library
   - Access metadata from within the library
   - Write tests for metadata validation

2. `sample_elf_bin`: ELF binary example showing how to:
   - Embed metadata into an executable
   - Access it via `get_module_info!`, `get_version()`, and `get_module_version()`

3. `sample_elf_bin_with_lib`: ELF binary that links `sample_lib` and
   demonstrates reading both the executable's and the library's metadata.

4. `sample_crashing_process`: tool that deliberately crashes with various signals so
   you can verify the `.note.package` section is preserved in the resulting
   core dump.

5. `sample_builder_api`: `build.rs` that uses the explicit builder API
   (`PackageMetadata` + `embed_package_metadata`) to supply metadata
   programmatically, bypassing the zero-config entry point.

6. `sample_info_api`: `build.rs` that uses the one-call
   `module_info::new(Info { ... })` entry point. Demonstrates the
   JSON-key-shaped struct literal (`r#type`, `moduleVersion`, `osVersion`) and
   the disable-fields pattern: `repo`, `branch`, `hash`, and `copyright` are
   left as the `Default` empty string via `..Default::default()`, so the
   embedded JSON ships those keys as `""`. Runtime tests assert both that the
   identity fields carry their supplied values and that the disabled fields
   come back empty.

Build and run (from the `module_info` crate root):

```bash
$ cargo build --manifest-path examples/sample_elf_bin/Cargo.toml
$ ./examples/sample_elf_bin/target/debug/sample_elf_bin
$ readelf -n ./examples/sample_elf_bin/target/debug/sample_elf_bin
```

Each example has its own `build.rs` and links like any downstream consumer,
the standalone-package layout sidesteps the linker-script ordering issues
that arise when using cargo `[[example]]` entries.

If you prefer not to clone the repo, copy any example's `Cargo.toml`,
`build.rs`, and `src/` into a new crate and build it there. That is the
same shape a real consumer uses.

## Technical Details

### ELF Note Section Format

The metadata is stored in the `.note.package` section of the ELF binary with:

- Owner name: "FDO"
- Type: `0xcafe1a7e`
- Content: JSON-formatted metadata

### Memory Alignment

All data is aligned to 4-byte boundaries for optimal binary compatibility.

### First-Page Placement

The linker script inserts `.note.package` after `.note.gnu.build-id`, which the linker places in the first page of the ELF image. This keeps the note visible in minimal coredumps that only capture the first read-only page. Verify with `readelf -l <bin>`; the note should appear in the first `LOAD` segment.

## CI Integration

CI pipelines typically supply build numbers via environment variables. Point `version_env_var_name` and `module_version_env_var_name` at the variable your pipeline exports; the crate reads the value at build time and embeds it into the note section.

For [Azure DevOps Pipelines](https://learn.microsoft.com/en-us/azure/devops/pipelines/build/variables) `Build.BuildNumber` is exposed to the build script as the `BUILD_BUILDNUMBER` environment variable; the example below uses that name. GitHub Actions exposes `GITHUB_RUN_NUMBER`, GitLab uses `CI_PIPELINE_IID`, etc. Point both keys at whatever your pipeline exports.

The embedded `moduleVersion` is intentionally separate from `Cargo.toml`'s `[package].version`. `[package].version` is the SemVer string crates.io and `cargo` use for dependency resolution; `moduleVersion` is the 4-part build identifier (e.g. `5.2.100.0`) that the build pipeline assigns and that crash-triage tools key on. They can be identical (the crate falls back to `[package].version` when the env var is unset), but in pipeline builds they normally diverge: `[package].version` stays at the released SemVer while `moduleVersion` carries the pipeline's incrementing build number.

```toml
[package.metadata.module_info]
version_env_var_name = "BUILD_BUILDNUMBER"        # Azure DevOps Pipelines
module_version_env_var_name = "BUILD_BUILDNUMBER" # or GITHUB_RUN_NUMBER, CI_PIPELINE_IID, etc.
```

If the variable is unset, the crate falls back to `Cargo.toml`'s `package.version`.

### Local reproduction

You can reproduce what the pipeline does locally by exporting the same
variable on the command line. The crate strips SemVer-style `-<prerelease>`
and `+<buildmeta>` suffixes before splitting on `.`, so pipeline-style
build numbers normalize cleanly:

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

Each dotted component must fit in a `u16` (0..=65535); out-of-range values
fail the build rather than silently wrap. A build number with fewer than
four numeric parts (`1.2.3`) is zero-padded on the right (`1.2.3.0`).

## Error Handling

The `module_info` crate uses a structured error handling approach with strongly-typed errors to help you handle different error cases appropriately. Import the error types directly:

```rust
use module_info::{get_module_info, ModuleInfoError, ModuleInfoResult};
```

### Error Types

All functions that can fail return a `ModuleInfoResult<T>` type, which is a type alias for `Result<T, ModuleInfoError>`. The `ModuleInfoError` enum provides the following variants:

| Error Variant | Description | When It Occurs |
|---------------|-------------|---------------|
| `NotAvailable(String)` | Module info is not available | When the "embed-module-info" feature is not enabled or the code is running on a non-Linux platform |
| `NullPointer` | A null pointer was encountered | When attempting to extract module info from a null pointer |
| `Utf8Error(std::str::Utf8Error)` | UTF-8 parsing error | When the binary contains module info that isn't valid UTF-8 |
| `MalformedJson(String)` | JSON format error | When the extracted metadata string doesn't follow the expected JSON format, or when build-time validation rejects a missing/empty required field or an out-of-range `moduleVersion` |
| `MetadataTooLarge(String)` | Metadata exceeded the `.note.package` JSON size limit | At build time when the serialized metadata JSON is larger than `MAX_JSON_SIZE` (1 KiB) |
| `IoError(std::io::Error)` | File I/O error | During build time when generating the linker script or reading from Cargo.toml |
| `Other(Box<dyn Error>)` | Unexpected errors | Catch-all for any other errors that might occur |

### Handling Errors

Here's an example of handling specific error types when retrieving module info.
Because `ModuleInfoError` is `#[non_exhaustive]`, the wildcard `Err(e)` arm at
the bottom is required; it also keeps you source-compatible when new variants
are added in future minor releases.

```rust
use module_info::{get_module_info, ModuleInfoError};

fn print_module_version() {
    match get_module_info!(ModuleInfoField::Version) {
        Ok(version) => println!("Module version: {}", version),

        // Handle specific error cases
        Err(ModuleInfoError::NotAvailable(msg)) => {
            eprintln!("Module info not available: {}", msg);
            eprintln!("Ensure you have enabled the 'embed-module-info' feature in your Cargo.toml");
        },
        Err(ModuleInfoError::NullPointer) => {
            eprintln!("Module version pointer is null");
            eprintln!("Check that your build process correctly integrated the module_info crate");
        },
        Err(ModuleInfoError::MalformedJson(msg)) => {
            eprintln!("Module version has invalid format: {}", msg);
        },

        // Required wildcard arm (#[non_exhaustive])
        Err(e) => eprintln!("Failed to get module version: {}", e),
    }
}
```

### Cross-Platform Error Handling

When using the `module_info` crate in cross-platform code, you should handle the `NotAvailable` error gracefully:

```rust
use module_info::{get_module_info, ModuleInfoError};

fn log_binary_info() {
    match get_module_info!(ModuleInfoField::Binary) {
        Ok(name) => log::info!("Running binary: {}", name),
        Err(ModuleInfoError::NotAvailable(_)) => {
            // On non-Linux platforms, this is expected behavior
            log::debug!("Module info not available on this platform");
        },
        Err(e) => log::warn!("Failed to get binary name: {}", e),
    }
}
```

### Error Contexts

Some errors, particularly the `MalformedJson` and `NotAvailable` variants, include additional context as a `String`. You can extract this information for more detailed error handling or logging:

```rust
match result {
    Err(ModuleInfoError::MalformedJson(details)) => {
        log::error!("JSON parsing error: {}", details);
        // Take appropriate action based on the specific error details
    },
    // Other cases...
}
```

## Security Considerations

The `module_info` crate includes several security features to protect against potential vulnerabilities:

### Metadata Validation

- All metadata is validated for size constraints before embedding
- A 1KB limit is enforced to prevent excessive memory consumption
- Size validation helps prevent potential denial of service vectors
- Checks that the seven required identity-plus-platform fields (`binary`, `version`, `moduleVersion`, `name`, `maintainer`, `os`, `osVersion`) are present and non-empty. The remaining fields (`type`, `repo`, `branch`, `hash`, `copyright`) may be left empty when a consumer deliberately opts out; see [Disabling fields](#disabling-fields).

### Input Sanitization

- All inputs to the note section creation are sanitized
- Control characters (including whitespace like `\n`, `\r`, `\t`) are stripped; the `.note.package` payload is pure printable ASCII

### JSON Validation

- Metadata JSON is validated for correct structure
- Required fields are checked to ensure they exist
- Malformed JSON is rejected with appropriate error messages

### Resource Limitation

- OS release file reading has a 10KB size limit to prevent resource exhaustion
- File operations use proper error handling and resource cleanup

### Recommendations

When using `module_info`:

1. Keep your metadata concise and minimal
2. Avoid including sensitive information in metadata fields

## Git Metadata Implementation

The `module_info` crate retrieves git repository information using git CLI commands at runtime.

### Git Information Collection

The module_info crate collects the following information about your git repository:

- **Branch name**: Retrieved using `git rev-parse --abbrev-ref HEAD`
- **Commit hash**: Retrieved using `git rev-parse HEAD`
- **Repository name**: Derived from `git remote get-url origin`. The last `/`- or `:`-separated path segment, with a trailing `.git` stripped (so `git@github.com:user/repo.git` → `repo`). Falls back to the project directory name when no remote is configured or git is unavailable.

This information is embedded in the note section of your binary and is also available at runtime through the `get_module_info!` macro.

### Requirements

- Git must be installed on the system where the code is running
- The code must be in a git repository
- If git is not available, the metadata will show "Unknown" for git-related fields

### Build-Script Context

`generate_project_metadata_and_linker_script()` and `embed_package_metadata()`
rely on Cargo-provided environment variables (`OUT_DIR`, `CARGO_MANIFEST_DIR`,
`CARGO_PKG_*`) that only exist inside a `build.rs` invocation. Calling them
from elsewhere either errors with `OUT_DIR` missing or silently falls back
to `"Unknown"`-shaped metadata. There is no explicit "I am a build script"
check; the functions don't need one because the Cargo-provided env vars
are the de-facto signal. Both entry points also emit
`cargo:rerun-if-changed=` / `cargo:rerun-if-env-changed=` directives, which
are no-ops outside a build-script context.

## References

- [ELF Format Specification](https://refspecs.linuxfoundation.org/elf/elf.pdf)
- [ELF Note Sections](https://docs.oracle.com/cd/E23824_01/html/819-0690/chapter6-18048.html)
- [Package metadata spec (UAPI Group)](https://uapi-group.org/specifications/specs/package_metadata_for_executable_files/)
- [Linux crash dumps with WinDbg](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/linux-crash-dumps)
- [Analyze crash dump files by using WinDbg](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/crash-dump-files)
- [Linux symbols and sources](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/linux-dwarf-symbols)
- [WinDbg Release notes](https://learn.microsoft.com/en-us/windows-hardware/drivers/debuggercmds/windbg-release-notes)

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE) for the full license text.

## Contributing

This project welcomes contributions and suggestions.  Most contributions require you to agree to a
Contributor License Agreement (CLA) declaring that you have the right to, and actually do, grant us
the rights to use your contribution. For details, visit [Contributor License Agreements](https://cla.opensource.microsoft.com).

When you submit a pull request, a CLA bot will automatically determine whether you need to provide
a CLA and decorate the PR appropriately (e.g., status check, comment). Simply follow the instructions
provided by the bot. You will only need to do this once across all repos using our CLA.

This project has adopted the [Microsoft Open Source Code of Conduct](https://opensource.microsoft.com/codeofconduct/).
For more information see the [Code of Conduct FAQ](https://opensource.microsoft.com/codeofconduct/faq/) or
contact [opencode@microsoft.com](mailto:opencode@microsoft.com) with any additional questions or comments.

## Trademarks

This project may contain trademarks or logos for projects, products, or services. Authorized use of Microsoft
trademarks or logos is subject to and must follow
[Microsoft's Trademark & Brand Guidelines](https://www.microsoft.com/legal/intellectualproperty/trademarks/usage/general).
Use of Microsoft trademarks or logos in modified versions of this project must not cause confusion or imply Microsoft sponsorship.
Any use of third-party trademarks or logos are subject to those third-party's policies.
