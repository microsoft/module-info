// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use module_info::get_module_info;
use std::ffi::CString;
use std::os::raw::c_char;
use std::sync::OnceLock;

module_info::embed!();

// `get_module_info!` reads the linker-script-defined `module_info_*` symbols
// from the *current ELF module*. Because this crate is built as a `cdylib`,
// those symbols are local to `libsample_lib.so` (they do not appear in the
// `.so`'s dynamic symbol table), so code inside the `.so` resolves them to
// the library's own `.note.package` data. A loader executable that also
// embeds `.note.package` does not displace the library's symbols at
// `dlopen` time.
//
// The exports below are `extern "C"` so a consumer can `dlopen` the `.so`
// and call into it. See `examples/sample_elf_bin_with_lib` for the loader.

#[no_mangle]
pub extern "C" fn sample_lib_print_info() {
    let binary = get_module_info!(ModuleInfoField::Binary).unwrap_or_default();
    let version = get_module_info!(ModuleInfoField::Version).unwrap_or_default();
    let module_version = get_module_info!(ModuleInfoField::ModuleVersion).unwrap_or_default();
    let maintainer = get_module_info!(ModuleInfoField::Maintainer).unwrap_or_default();
    println!("  Binary: {binary}");
    println!("  Version: {version}");
    println!("  Module Version: {module_version}");
    println!("  Maintainer: {maintainer}");
}

#[no_mangle]
pub extern "C" fn sample_lib_binary_name() -> *const c_char {
    static CSTR: OnceLock<CString> = OnceLock::new();
    CSTR.get_or_init(|| {
        let s = get_module_info!(ModuleInfoField::Binary).unwrap_or_default();
        CString::new(s).unwrap_or_default()
    })
    .as_ptr()
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
