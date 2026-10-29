# Module Info Guide

Start with the [README](../README.md). This guide covers configuration and
advanced use. See the [API reference](https://docs.rs/module-info) for signatures.

## Metadata fields

The `.note.package` section contains JSON. Every key is present. Seven fields
must be non-empty; optional fields can be set to `""`.

| Field | Default source | Required |
|-------|----------------|----------|
| `binary`, `name` | Cargo package name | Yes |
| `version` | Cargo version, formatted to three parts | Yes |
| `moduleVersion` | Version, formatted to four u16 parts | Yes |
| `maintainer` | Package metadata, or `"Unknown"` | Yes |
| `os`, `osVersion` | Build host's `/etc/os-release` | Yes |
| `type`, `copyright` | Package metadata, or `"Unknown"` | No |
| `repo` | Git origin's repository name, or project directory name | No |
| `branch`, `hash` | Git, or `"unknown"` | No |

`moduleVersion` parts must be between 0 and 65535. Required values that become
empty after sanitization fail validation.

## Configuration

### Static values in `Cargo.toml`

These are the supported package-metadata settings:

```toml
[package.metadata.module_info]
maintainer = "team@contoso.com"
type = "tool"
copyright = "Contoso, Ltd."
version_env_var_name = "BUILD_BUILDNUMBER"
module_version_env_var_name = "BUILD_BUILDNUMBER"
allow_prerelease_suffix = false
```

All settings are optional. Use the builder API below to override other fields.
`type` is a free-form label. Omitting `type` or `copyright` produces `"Unknown"`,
not an empty string.

### Environment variables (CI build numbers)

The two `*_env_var_name` settings name environment variables to read, not
literal versions. Unset or whitespace-only values use the fallback version.
If no separate module version is provided, it uses the resolved `version`.

Missing numeric parts are padded with zero; extra parts are dropped with a
warning. For example, `BUILD_BUILDNUMBER=5.2.100.42` produces `version=5.2.100`
and `moduleVersion=5.2.100.42`.

### Preserving SemVer-style pre-release / build suffixes

By default, text starting at the first `-` or `+` is removed. Set
`allow_prerelease_suffix = true` to retain it after formatting the numeric core.
For example, `2.1.0-beta.3` then gives `moduleVersion=2.1.0.0-beta.3`.
Only enable this when your consumers accept suffixes.

### Disabling optional fields

Clear optional fields after collection:

```rust,no_run
use module_info::{embed_package_metadata, EmbedOptions, PackageMetadata};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut md = PackageMetadata::from_cargo_toml()?;
    md.repo.clear();
    md.branch.clear();
    md.hash.clear();
    let _artifacts = embed_package_metadata(&md, &EmbedOptions::default())?;
    Ok(())
}
```

### Debug output

Set `MODULE_INFO_DEBUG=true` for detailed build-script diagnostics.

## Custom `build.rs`: supplying metadata programmatically

### Option A: one-call struct literal via `module_info::new(Info { … })`

Use `Info` when you supply every value yourself:

```rust,no_run
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _artifacts = module_info::new(module_info::Info {
        binary: "my_tool".into(),
        name: "my_tool".into(),
        maintainer: "team@contoso.com".into(),
        version: "1.2.3".into(),
        moduleVersion: "1.2.3.4".into(),
        os: "linux".into(),
        osVersion: "unknown".into(),
        ..Default::default()
    })?;
    Ok(())
}
```

This does not detect Git or OS values. Values are still sanitized and validated.
Use `..Default::default()` to allow future fields without changing your literal.

### Option B: `PackageMetadata` + `EmbedOptions`

Use `PackageMetadata::from_cargo_toml()` to collect defaults, then change fields.
Alternatively, start with `PackageMetadata::default()` and fill all required
fields. Construct `PackageMetadata` and `EmbedOptions` through `Default` and
field assignment; their non-exhaustive types forbid external struct literals.

For an outer build system that performs the final link:

```rust,no_run
use module_info::{embed_package_metadata, EmbedOptions, PackageMetadata};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let md = PackageMetadata::from_cargo_toml()?;
    let mut opts = EmbedOptions::default();
    opts.out_dir = Some(std::path::PathBuf::from(std::env::var("OUT_DIR")?));
    opts.emit_cargo_link_arg = false;
    let artifacts = embed_package_metadata(&md, &opts)?;
    println!("cargo:warning=Use {} at the final link", artifacts.linker_script_path.display());
    Ok(())
}
```

The outer linker must receive `-T<path-to-linker_script.ld>`. The final object
must also include the crate's note anchor, normally via `embed!()`.
The returned artifacts include the script, JSON, and raw note bytes.

## CI integration

Set the configured version variables in your pipeline before running Cargo:

```sh
BUILD_BUILDNUMBER=5.2.100.42 cargo build
```

Version variables named in package metadata are tracked for rebuilds. If your
custom `build.rs` reads other variables or files, emit the corresponding
`cargo:rerun-if-env-changed` or `cargo:rerun-if-changed` directives.

## Cross-compilation

Embedding supports little-endian Linux targets, including x86_64, aarch64, and
i686. Linux-to-Linux cross-builds need a matching Rust target and linker.
Auto-detected OS fields describe the build host; override them for the target.

Native Windows/macOS builds using the zero-config entry point are no-ops, and
runtime reads return `NotAvailable`. The explicit builder types are Linux-only.
Do not assume cross-host builds work: a Linux host targeting Windows still
emits an ELF script; a non-Linux host targeting Linux does not emit one.
Big-endian Linux notes are not supported.

## Validation and inspection

```sh
readelf -SW target/debug/your_binary
readelf -n target/debug/your_binary
```

Check that `.note.package` has type `NOTE` and the allocated (`A`) flag.
Binutils 2.39+ decodes its JSON. The note owner is `FDO`, its type is
`0xcafe1a7e`, and alignment is four bytes. Normal symbol stripping preserves it.

GNU ld.bfd supports the script; gold rejects `INSERT AFTER`. Other linkers
must be verified with the commands above. The insertion also requires
`.note.gnu.build-id`; disabling build IDs can break the link.

### Reading from a core dump

Recovery needs the note's memory in the dump. Verify its placement with
`readelf -lW`: the note should fit within the first page of its file-backed
mapping. Being somewhere in the first `LOAD` segment alone is not enough.
The kernel's `coredump_filter` and your dump collector decide what is captured.

A raw core has no section headers, so `readelf -n core` does not directly show
the embedded package note. Search the captured bytes instead:

```sh
strings core.dump | grep -oE '"moduleVersion":"[^"]+"' | cut -d'"' -f4
```

On systemd hosts, `coredumpctl dump <match> --output core.dump` exports the dump.
Package metadata may also appear in the journal, depending on the collector.
Use the existing crash example only in a suitable test environment.

## Libraries

- A `cdylib` runs its own link step and can read its own metadata.
- An `rlib` or statically linked library reads the final executable's values.
  Cargo does not forward a dependency's linker script to the final executable.
- An outer build can apply a static library's script at the final link.
  That does not create independent runtime metadata for every static library.

## Examples

The [examples directory](https://github.com/microsoft/module-info/tree/main/examples)
has standalone packages for basic embedding, builder APIs, shared libraries,
and crash dumps. Clone the repository to run them; they are not in the
published crate.

```sh
cargo test --manifest-path examples/sample_elf_bin/Cargo.toml
```

## Troubleshooting

| Symptom | Check |
|---------|-------|
| Missing note or `PROGBITS` type | Enable `embed-module-info`, add `build.rs` and `embed!()`, and verify the linker. |
| Undefined `module_info_*` symbols | Ensure the generated script reaches the final link. This is a link error, not a runtime fallback. |
| Macro fails when qualified or renamed | Import `use module_info::get_module_info;` and call it under that name. |
| `MetadataTooLarge` | Shorten fields; the complete JSON limit is 1 KiB. |
| Invalid `moduleVersion` | Use four numeric parts in 0..=65535, with only supported suffix handling. |
| Missing `osVersion` | Supply it through the builder API if the host's OS release file has no version. |
| Stale Git fields or repeated builds | Git watches assume a package-local `.git`; nested packages and worktrees need care. Verify the generated JSON. |

## Error handling

Propagate build errors with `?`. Runtime helpers return `ModuleInfoResult`.
Feature-off or non-Linux runtime access returns `NotAvailable`.
The all-field macro omits fields whose reads fail; a disabled optional field
still appears as `""`. `print_module_info` checks required fields.

Prefer safe accessors over `extract_module_info`. Its caller must ensure
pointer validity and readable memory; a scan limit cannot protect an invalid
pointer. See the [API error and safety contracts](https://docs.rs/module-info).

## Security considerations

Metadata is readable by anyone with the binary or dump. Do not embed secrets.
Values become printable ASCII: `©`, `®`, and `™` become `(c)`, `(r)`, and `(tm)`;
other non-ASCII characters, quotes, backslashes, and control characters are
removed. Dropped characters produce build warnings.

## References

- [Package metadata specification](https://uapi-group.org/specifications/specs/package_metadata_for_executable_files/)
- [Contributor checks](https://github.com/microsoft/module-info/blob/main/CONTRIBUTING.md)
