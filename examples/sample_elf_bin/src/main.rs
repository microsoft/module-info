// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use module_info::get_module_info;

module_info::embed!();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Print this executable's metadata
    println!("Executable Information:");
    let binary = get_module_info!(ModuleInfoField::Binary)?;
    let version = get_module_info!(ModuleInfoField::Version)?;
    println!("  Binary: {binary}");
    println!("  Version: {version}");

    // Demonstrate the direct accessor helpers.
    println!("  get_version(): {}", module_info::get_version()?);
    println!(
        "  get_module_version(): {}",
        module_info::get_module_version()?
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    // `module-info` is a regular dependency of this example with
    // `embed-module-info` always enabled, so no cfg gate is needed here.
    // Macros aren't re-exported by `use super::*`, so the runtime API is
    // imported explicitly.
    use module_info::get_module_info;

    #[test]
    fn test_metadata() -> Result<(), Box<dyn std::error::Error>> {
        let binary = get_module_info!(ModuleInfoField::Binary)?;
        assert_eq!(binary, "sample_elf_bin");
        Ok(())
    }
}
