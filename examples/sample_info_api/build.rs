// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! One-call-API example: construct an `Info` struct literal and hand it to
//! `module_info::new`, which converts it to `PackageMetadata` and embeds the
//! note section with `EmbedOptions::default()`.
//!
//! Use this shape when you want the terseness of a single struct literal and
//! don't need to customize `EmbedOptions` (custom `out_dir`,
//! suppressed `cargo:rustc-link-arg`, …). If you need those knobs, go through
//! `PackageMetadata` + `embed_package_metadata` instead; see
//! `sample_builder_api` for that flow.
//!
//! This example also demonstrates the "disable fields" pattern: `repo`,
//! `branch`, `hash`, and `copyright` are left as their `Default` empty string
//! via `..Default::default()`, so the embedded JSON ships those keys as `""`
//! even though the crate lives in a git repository. The seven required keys
//! (`binary`, `version`, `moduleVersion`, `name`, `maintainer`, `os`,
//! `osVersion`) must all be populated; this example sets them explicitly
//! since it doesn't read `/etc/os-release` via `from_cargo_toml`.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Populate all seven required keys plus `r#type`. Everything else
    // (`repo`, `branch`, `hash`, `copyright`) is filled in by
    // `..Default::default()` as empty strings, and because they aren't in
    // `REQUIRED_JSON_KEYS` the build still succeeds. Downstream tooling
    // reads those as "disabled."
    let artifacts = module_info::new(module_info::Info {
        binary: "sample_info_api".into(),
        name: "sample_info_api".into(),
        maintainer: "info-api-demo@contoso.com".into(),
        r#type: "tool".into(),
        version: "3.1.4".into(),
        moduleVersion: "3.1.4.159".into(),
        os: "linux".into(),
        osVersion: "unknown".into(),
        ..Default::default()
    })?;

    // Surface what we wrote so `cargo build -v` users can find the files.
    module_info::note!(
        "linker script at {}",
        artifacts.linker_script_path.display()
    );
    module_info::note!("module_info.json at {}", artifacts.json_path.display());

    Ok(())
}
