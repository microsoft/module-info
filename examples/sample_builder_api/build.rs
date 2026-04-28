// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Builder-API example: construct a `PackageMetadata` programmatically and
//! embed it via `embed_package_metadata`, rather than going through the
//! zero-config `generate_project_metadata_and_linker_script()` entry point.
//!
//! Two flows this supports:
//! 1. Supply metadata from build.rs without editing Cargo.toml: every field
//!    can be overridden at build time (useful when the metadata lives in an
//!    external manifest or build-pipeline variables).
//! 2. Static-library flows where the final link happens later: set
//!    `emit_cargo_link_arg = false` and the outer build system passes
//!    `linker_script.ld` to its own linker.
//!
//! This example covers the first flow: it starts from the Cargo.toml-driven
//! defaults, then overrides a handful of fields before embedding.

use module_info::{embed_package_metadata, EmbedOptions, PackageMetadata};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Start from the Cargo.toml-driven defaults so git info, OS detection, and
    // any `[package.metadata.module_info]` keys are all populated. Then
    // override whatever you want to set programmatically.
    let mut md = PackageMetadata::from_cargo_toml()?;

    // Overrides demonstrating build-time customization. Real consumers would
    // pull these from pipeline variables, an external manifest, etc.
    md.maintainer = "builder-api-demo@contoso.com".to_string();
    md.module_type = "tool".to_string();
    md.version = "2.0.0".to_string();
    md.module_version = "2.0.0.42".to_string();
    md.copyright = "Contoso, Ltd.".to_string();

    // `EmbedOptions` is #[non_exhaustive]; construct via Default and assign
    // fields rather than struct-literal syntax so future additions don't
    // break this build script. Keep `emit_cargo_link_arg = true` so cargo
    // actually links the generated script; a static-library build would set
    // both `out_dir = Some(custom_dir)` and `emit_cargo_link_arg = false`.
    let opts = EmbedOptions::default();

    let artifacts = embed_package_metadata(&md, &opts)?;

    // Surface what we wrote so `cargo build -v` users can find the files.
    module_info::note!(
        "linker script at {}",
        artifacts.linker_script_path.display()
    );
    module_info::note!("module_info.json at {}", artifacts.json_path.display());

    Ok(())
}
