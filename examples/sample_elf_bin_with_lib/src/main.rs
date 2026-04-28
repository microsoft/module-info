// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use module_info::get_module_info;

module_info::embed!();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Print this executable's metadata
    println!("Executable Information:");
    let binary = get_module_info!(ModuleInfoField::Binary)?;
    let version = get_module_info!(ModuleInfoField::Version)?;
    println!("  Binary: {}", binary);
    println!("  Version: {}", version);

    // Print the library's metadata
    println!("\nLinked Library Information:");
    let lib_info = sample_lib::print_lib_info()?;
    println!("  Loaded: {}", lib_info);

    Ok(())
}

#[cfg(test)]
mod tests {
    use module_info::get_module_info;

    #[test]
    fn test_metadata() -> Result<(), Box<dyn std::error::Error>> {
        let binary = get_module_info!(ModuleInfoField::Binary)?;
        assert_eq!(binary, "sample_elf_bin_with_lib");
        Ok(())
    }
}
