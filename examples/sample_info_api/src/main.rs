// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Runtime half of the one-call-API example. The build.rs for this crate
//! calls `module_info::new(Info { ... })` with `repo`, `branch`, `hash`, and
//! `copyright` left empty. This binary reads the note section back and
//! prints each field so you can see the "disabled" fields ship as empty
//! strings while the identity fields carry their supplied values.

use module_info::get_module_info;

module_info::embed!();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "sample_info_api: embedded metadata (all supplied via `module_info::new(Info {{..}})`):"
    );

    let binary = get_module_info!(ModuleInfoField::Binary)?;
    let version = get_module_info!(ModuleInfoField::Version)?;
    let module_version = get_module_info!(ModuleInfoField::ModuleVersion)?;
    let name = get_module_info!(ModuleInfoField::Name)?;
    let maintainer = get_module_info!(ModuleInfoField::Maintainer)?;
    let module_type = get_module_info!(ModuleInfoField::Type)?;

    println!("  binary:         {binary}");
    println!("  name:           {name}");
    println!("  version:        {version}");
    println!("  moduleVersion:  {module_version}");
    println!("  maintainer:     {maintainer}");
    println!("  type:           {module_type}");

    // Optional fields left empty at embed time. The single-field form
    // returns `Ok("")` and the no-arg HashMap form keeps every key with
    // its empty-string value. Either way, "" means "disabled."
    println!("\nOptional fields left empty in build.rs:");
    let repo = get_module_info!(ModuleInfoField::Repo).unwrap_or_default();
    let branch = get_module_info!(ModuleInfoField::Branch).unwrap_or_default();
    let hash = get_module_info!(ModuleInfoField::Hash).unwrap_or_default();
    let copyright = get_module_info!(ModuleInfoField::Copyright).unwrap_or_default();
    println!("  repo:           {:?}", repo);
    println!("  branch:         {:?}", branch);
    println!("  hash:           {:?}", hash);
    println!("  copyright:      {:?}", copyright);

    Ok(())
}

#[cfg(test)]
mod tests {
    // `module-info` is a regular (non-feature) dependency with
    // `embed-module-info` permanently enabled on *this* crate, so the note
    // section is always present; no `#[cfg(feature = ...)]` gate needed.
    // `use module_info::get_module_info;` must be written inside the test
    // module explicitly: macros aren't re-exported by `use super::*`.
    use module_info::get_module_info;

    #[test]
    fn identity_fields_land_via_new_info() {
        let binary = match get_module_info!(ModuleInfoField::Binary) {
            Ok(v) => v,
            Err(e) => panic!("binary field should be readable: {e}"),
        };
        assert_eq!(binary, "sample_info_api");

        let version = match get_module_info!(ModuleInfoField::Version) {
            Ok(v) => v,
            Err(e) => panic!("version field should be readable: {e}"),
        };
        assert_eq!(version, "3.1.4");

        let module_version = match get_module_info!(ModuleInfoField::ModuleVersion) {
            Ok(v) => v,
            Err(e) => panic!("moduleVersion field should be readable: {e}"),
        };
        assert_eq!(module_version, "3.1.4.159");

        let maintainer = match get_module_info!(ModuleInfoField::Maintainer) {
            Ok(v) => v,
            Err(e) => panic!("maintainer field should be readable: {e}"),
        };
        assert_eq!(maintainer, "info-api-demo@contoso.com");
    }

    /// The no-arg `get_module_info!()` map form must carry *every* key,
    /// including the disabled ones, not just the populated keys. This is
    /// the documented contract (see README "Disabling fields") and is the
    /// one checked against H5: without this test, a future refactor that
    /// re-introduces a `!value.is_empty()` filter in the macro's
    /// `@__add_to_map` arm would silently break downstream consumers that
    /// round-trip through the map form.
    #[test]
    fn map_form_carries_every_key_including_disabled() -> Result<(), Box<dyn std::error::Error>> {
        let info = get_module_info!()?;
        for expected_key in [
            "binary",
            "version",
            "moduleVersion",
            "name",
            "maintainer",
            "type",
            "repo",
            "branch",
            "hash",
            "copyright",
            "os",
            "osVersion",
        ] {
            assert!(
                info.contains_key(expected_key),
                "map form must contain {expected_key:?} even when the field is disabled (value is empty string)"
            );
        }
        // The four fields deliberately left empty in build.rs must be
        // present in the map with an empty-string value, not dropped.
        for disabled in ["repo", "branch", "hash", "copyright"] {
            let value = info
                .get(disabled)
                .map(String::as_str)
                .unwrap_or("<missing>");
            assert_eq!(
                value, "",
                "{disabled} should be present-but-empty in the map form"
            );
        }
        Ok(())
    }

    /// The disable pattern in build.rs (leaving `repo`/`branch`/`hash`/
    /// `copyright` as `..Default::default()`) must produce empty strings
    /// at runtime. If these ever start surfacing git-derived values, the
    /// one-call API is silently picking them up somewhere it shouldn't.
    #[test]
    fn disabled_fields_are_empty() {
        let repo = get_module_info!(ModuleInfoField::Repo).unwrap_or_default();
        assert!(repo.is_empty(), "repo should be disabled, got {repo:?}");

        let branch = get_module_info!(ModuleInfoField::Branch).unwrap_or_default();
        assert!(
            branch.is_empty(),
            "branch should be disabled, got {branch:?}"
        );

        let hash = get_module_info!(ModuleInfoField::Hash).unwrap_or_default();
        assert!(hash.is_empty(), "hash should be disabled, got {hash:?}");

        let copyright = get_module_info!(ModuleInfoField::Copyright).unwrap_or_default();
        assert!(
            copyright.is_empty(),
            "copyright should be disabled, got {copyright:?}"
        );
    }
}
