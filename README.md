# Module Info

Embed build-time metadata (version, git commit, maintainer, OS) into your
Rust binary so **it survives crashes**. The data lives in an ELF
`.note.package` section, so when your process dies and dumps core, the metadata
travels with it. Tools like `coredumpctl`, `readelf -n`, and GDB see exactly
which build crashed, even if the binary has been redeployed or deleted.

[![crates.io](https://img.shields.io/crates/v/module-info.svg)](https://crates.io/crates/module-info)
[![Documentation](https://docs.rs/module-info/badge.svg)](https://docs.rs/module-info)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## Why module-info?

When a deployed binary crashes, the hardest question is often "which build was
this?" Symbol files get lost, tags drift, and the binary on disk may already be
gone. `module-info` answers it from the core dump alone:

- **Survives the crash.** The metadata is an ELF note baked into the image, so
  it is captured in the core dump even if the binary has been redeployed or
  deleted.
- **Standard, tool-ready format.** Follows the [systemd package-metadata
  spec](https://uapi-group.org/specifications/specs/package_metadata_for_executable_files/),
  so `coredumpctl`, `readelf -n`, GDB, and existing crash-analysis tooling read
  it with no changes.
- **Auto-detected.** Version, git branch/commit/repo, OS, and package details
  are collected at build time from `Cargo.toml`, git, and the OS. A typical
  setup needs three lines.
- **Build-time only.** The note is written during the build, so carrying it
  costs nothing at runtime. Reading the metadata back from inside the running
  process is optional and lives behind the `embed-module-info` feature.
- **Executables and shared libraries.** Both carry their own `.note.package`.
- **Cross-platform safe.** Compiles everywhere; it is a no-op on non-Linux
  targets, so the same source builds on Windows and macOS.

## What gets embedded

A small JSON record in the binary's `.note.package` section, one key/value pair
per line and ASCII-only (see [Limitations](#limitations)):

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

Reading it back needs no special tooling. On binutils 2.39+, `readelf -n`
decodes the note directly:

```sh
$ readelf -n ./sample_crashing_process
...
Displaying notes found in: .note.package
  Owner                 Data size   Description
  FDO                  0x00000144   FDO_PACKAGING_METADATA
    Packaging Metadata: {"binary":"sample_crashing_process","moduleVersion":"0.1.0.0", ... }
```

(The exact `Data size` depends on your field values.)

## Quick start

**1. Add the dependency.**

```sh
cargo add module-info --features embed-module-info
cargo add --build module-info
```

Requires Rust 1.74 or newer.

**2. Add a `build.rs`** at your project root:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    module_info::generate_project_metadata_and_linker_script()?;
    Ok(())
}
```

**3. Embed the note** by adding one macro at your crate root (`src/main.rs` or
`src/lib.rs`):

```rust
module_info::embed!();
```

That's it. Your binary now carries its metadata. Most fields are detected
automatically from `Cargo.toml`, git, and the OS. To set the maintainer and a
few extras, add this to `Cargo.toml`:

```toml
[package.metadata.module_info]
maintainer = "team@contoso.com"   # contact email or team UUID
type = "agent"                    # optional
copyright = "Contoso, Ltd."       # optional
```

**How the three pieces fit.** The `[build-dependencies]` entry runs the build
script, which collects the metadata and tells the linker to add the
`.note.package` section. The `[dependencies]` entry plus the `embed-module-info`
feature anchor that section in the final binary and provide the runtime
read-back API. `module_info::embed!()` is that anchor; it expands to nothing on
non-Linux targets, so the same source builds everywhere. In a workspace, each
binary or shared library that should carry metadata needs its own `build.rs`
and `embed!()` call.

### Read it back at runtime (optional)

```rust
use module_info::get_module_info;

if let Ok(version) = get_module_info!(ModuleInfoField::Version) {
    println!("Version: {version}");
}

// Or grab everything as a HashMap:
if let Ok(all) = get_module_info!() {
    for (key, value) in all {
        println!("{key}: {value}");
    }
}
```

`ModuleInfoField` does not need importing: the `get_module_info!` macro matches
the variant name (e.g. `ModuleInfoField::Version`) as a token.

Runtime read-back is a convenience; the main feature is that the metadata is
recoverable from a crash dump without it.

## Platform support

- **Linux:** full functionality. Emits the `.note.package` section at build
  time; runtime accessors read it back.
- **Windows / macOS / other:** no-op. Nothing is embedded, build-script entry
  points compile to empty stubs (so the same source builds everywhere), and the
  runtime accessors return `ModuleInfoError::NotAvailable`, so handle that in
  cross-platform code.

## Verify it worked

```sh
$ readelf -n ./your_binary        # binutils >= 2.39 prints the JSON directly
$ objcopy --dump-section .note.package=/dev/stdout ./your_binary
```

`.note.package` is an allocated section, so it survives `strip`.

## Reading metadata from a core dump

This is the payoff: because the note sits in the first read-only page, it is
captured in the core dump even when the binary is gone. A core has no section
headers, so pull a field straight out of the dumped bytes with `strings`:

```sh
$ strings core | grep -oE '"moduleVersion":"[^"]+"'
"moduleVersion":"0.1.0.0"
```

Or use `coredumpctl` on systemd hosts. The [full
guide](docs/GUIDE.md#reading-from-a-core-dump) has the full commands, and the
`sample_crashing_process` example exercises it end to end.

## Limitations

- **Linux/ELF only.** On other targets embedding is a no-op (the build still
  compiles) and the runtime accessors return `ModuleInfoError::NotAvailable`.
- **GNU ld / gold linker.** The note is placed with an `INSERT AFTER` linker
  script. Alternative linkers (`lld`, `mold`) may not honor it; if you use one,
  confirm the section is present with `readelf -n`.
- **ASCII-only fields.** Values are sanitized to printable ASCII. `©`/`®`/`™`
  become `(c)`/`(r)`/`(tm)`; all other non-ASCII (accents, curly quotes, CJK)
  is dropped, so prefer ASCII spellings at the source.
- **1 KiB cap.** The whole JSON record must fit in 1 KiB or the build fails
  with `MetadataTooLarge`.
- **Requires Rust 1.74+** and a Cargo `build.rs` context.

## Learn more

The [**full guide**](docs/GUIDE.md) covers everything beyond the basics:

- [Metadata fields](docs/GUIDE.md#metadata-fields) and
  [configuration](docs/GUIDE.md#configuration)
- [CI integration](docs/GUIDE.md#ci-integration): wiring up build numbers from
  Azure Pipelines, GitHub Actions, etc.
- [Custom `build.rs`](docs/GUIDE.md#custom-buildrs-supplying-metadata-programmatically):
  supplying metadata programmatically and static-library flows
- [Disabling optional fields](docs/GUIDE.md#disabling-optional-fields),
  [cross-compilation](docs/GUIDE.md#cross-compilation),
  [inspection](docs/GUIDE.md#validation-and-inspection),
  [error handling](docs/GUIDE.md#error-handling), and
  [security](docs/GUIDE.md#security-considerations)

API reference: [docs.rs/module-info](https://docs.rs/module-info).

Changelog: [CHANGELOG.md](CHANGELOG.md). Security policy: [SECURITY.md](SECURITY.md).

## License

This project is licensed under the MIT License. See [LICENSE.txt](LICENSE.txt)
for the full license text.

## Contributing

This project welcomes contributions and suggestions. Most contributions require
you to agree to a Contributor License Agreement (CLA) declaring that you have
the right to, and actually do, grant us the rights to use your contribution.
For details, visit [Contributor License Agreements](https://cla.opensource.microsoft.com).

When you submit a pull request, a CLA bot will automatically determine whether
you need to provide a CLA and decorate the PR appropriately (e.g., status
check, comment). Simply follow the instructions provided by the bot. You will
only need to do this once across all repos using our CLA.

This project has adopted the [Microsoft Open Source Code of Conduct](https://opensource.microsoft.com/codeofconduct/).
For more information see the [Code of Conduct FAQ](https://opensource.microsoft.com/codeofconduct/faq/) or
contact [opencode@microsoft.com](mailto:opencode@microsoft.com) with any additional questions or comments.

## Trademarks

This project may contain trademarks or logos for projects, products, or
services. Authorized use of Microsoft trademarks or logos is subject to and
must follow [Microsoft's Trademark & Brand Guidelines](https://www.microsoft.com/legal/intellectualproperty/trademarks/usage/general).
Use of Microsoft trademarks or logos in modified versions of this project must
not cause confusion or imply Microsoft sponsorship. Any use of third-party
trademarks or logos are subject to those third-party's policies.
