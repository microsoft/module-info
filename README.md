# Module Info

**Identify which Rust build crashed on Linux.**

`module-info` stores the version, Git commit, and other build details in an
ELF note: a metadata section in a Linux binary. Read it without debug symbols,
or recover it from a core dump that captured the section.

[![crates.io](https://img.shields.io/crates/v/module-info.svg)](https://crates.io/crates/module-info)
[![Documentation](https://docs.rs/module-info/badge.svg)](https://docs.rs/module-info)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## Quick start

Requires Rust 1.85+ and a supported Linux build. In your crate:

**1. Add runtime and build dependencies.**

```sh
cargo add module-info --features embed-module-info
cargo add --build module-info
```

**2. Create `build.rs`:**

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    module_info::generate_project_metadata_and_linker_script()?;
    Ok(())
}
```

**3. Add this at the top of `src/main.rs` or `src/lib.rs`:**

```rust
module_info::embed!();
```

The build collects package, Git, and build-host OS details automatically.
Set a maintainer in `Cargo.toml`:

```toml
[package.metadata.module_info]
maintainer = "team@contoso.com"
```

Each executable or shared-library package needs its own setup.

## Verify it worked

```sh
cargo build
readelf -n target/debug/your_binary
```

With binutils 2.39+, look for:

```text
Displaying notes found in: .note.package
...
    Packaging Metadata: ...
```

For crash analysis, see [reading a core dump](docs/GUIDE.md#reading-from-a-core-dump).

## Read the version at runtime

Runtime access is optional:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Version: {}", module_info::get_version()?);
    Ok(())
}
```

## Limitations

- Embedding supports little-endian Linux targets. Native non-Linux builds
  have no metadata; cross-host builds have [additional limits](docs/GUIDE.md#cross-compilation).
- GNU ld.bfd is supported; gold cannot parse the script. Verify other linkers
  with `readelf -SW`: `.note.package` must have type `NOTE`.
- Values become printable ASCII; unsupported characters are removed.
  The entire JSON record must fit in 1 KiB.
- Statically linked libraries read the executable's metadata, not their own.
- Core-dump recovery depends on the dump capturing the note's memory.
- Metadata is public. Do not embed secrets.

## Learn more

- [Guide](docs/GUIDE.md): fields, CI versions, custom metadata, and troubleshooting.
- [API reference](https://docs.rs/module-info) and [examples](https://github.com/microsoft/module-info/tree/main/examples).
- [Changelog](CHANGELOG.md) and [security policy](https://github.com/microsoft/module-info/blob/main/SECURITY.md).

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
