// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use module_info::get_module_info;

module_info::embed!();

// NOTE: `get_module_info!` reads the `.note.package` symbols that the linker
// places in the *current ELF module*. When this crate is linked as an rlib
// into an executable (the usual case, and what `sample_elf_bin_with_lib`
// does), the symbols resolve to the executable's metadata, not the library's.
// The same is true for `cdylib` consumers loaded with `dlopen`: the dynamic
// linker picks one definition per symbol and the main executable typically
// wins. See the crate-level "Limitations" section in `lib.rs`.
pub fn print_lib_info() -> Result<String, Box<dyn std::error::Error>> {
    let binary: String = get_module_info!(ModuleInfoField::Binary)?;
    let version: String = get_module_info!(ModuleInfoField::Version)?;
    let module_version: String = get_module_info!(ModuleInfoField::ModuleVersion)?;
    let maintainer: String = get_module_info!(ModuleInfoField::Maintainer)?;

    println!("Library view (shares the executable's symbols; see crate docs):");
    println!("  Binary: {binary}");
    println!("  Version: {version}");
    println!("  Module Version: {module_version}");
    println!("  Maintainer: {maintainer}");

    Ok(format!("{binary} v{version}"))
}

pub fn get_version() -> Result<String, Box<dyn std::error::Error>> {
    let version: String = get_module_info!(ModuleInfoField::Version)?;
    Ok(version)
}

pub fn get_module_version() -> Result<String, Box<dyn std::error::Error>> {
    let version: String = get_module_info!(ModuleInfoField::ModuleVersion)?;
    Ok(version)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn test_metadata() -> TestResult {
        let binary = get_module_info!(ModuleInfoField::Binary)?;
        assert_eq!(binary, "sample_lib");

        let maintainer = get_module_info!(ModuleInfoField::Maintainer)?;
        assert_eq!(maintainer, "example@contoso.com");
        Ok(())
    }
}
