// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Runtime half of the builder-API example. The build.rs for this crate
//! constructs a `PackageMetadata` programmatically and overrides version,
//! module_version, maintainer, and copyright. This binary reads those back
//! out of the embedded `.note.package` section to prove the override flowed
//! through to the final ELF.

use module_info::get_module_info;

module_info::embed!();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("sample_builder_api: embedded metadata (overrides set in build.rs):");

    let binary = get_module_info!(ModuleInfoField::Binary)?;
    let version = get_module_info!(ModuleInfoField::Version)?;
    let module_version = get_module_info!(ModuleInfoField::ModuleVersion)?;
    let maintainer = get_module_info!(ModuleInfoField::Maintainer)?;
    let copyright = get_module_info!(ModuleInfoField::Copyright)?;

    println!("  binary:         {binary}");
    println!("  version:        {version}         (build.rs set 2.0.0)");
    println!("  moduleVersion:  {module_version}   (build.rs set 2.0.0.42)");
    println!("  maintainer:     {maintainer}");
    println!("  copyright:      {copyright}");

    // The public accessor functions go through the same extern-symbol path.
    println!("\nAccessor functions:");
    println!("  get_version():         {}", module_info::get_version()?);
    println!(
        "  get_module_version():  {}",
        module_info::get_module_version()?
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use module_info::get_module_info;

    #[test]
    fn builder_overrides_reach_the_note_section() -> Result<(), Box<dyn std::error::Error>> {
        let version = get_module_info!(ModuleInfoField::Version)?;
        assert_eq!(
            version, "2.0.0",
            "build.rs override for version didn't land"
        );

        let module_version = get_module_info!(ModuleInfoField::ModuleVersion)?;
        assert_eq!(
            module_version, "2.0.0.42",
            "build.rs override for moduleVersion didn't land"
        );

        let maintainer = get_module_info!(ModuleInfoField::Maintainer)?;
        assert_eq!(maintainer, "builder-api-demo@contoso.com");
        Ok(())
    }
}
