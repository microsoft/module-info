// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use module_info::get_module_info;
use std::env;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::path::PathBuf;

module_info::embed!();

fn default_lib_path() -> PathBuf {
    let here = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    // Match the profile this binary was built with: a `cargo build --release`
    // of the loader expects to find sample_lib's release artifact, not debug.
    // `cfg!(debug_assertions)` is the standard way to discriminate at runtime
    // without a build-script-set env var.
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    here.join(format!(
        "../../../sample_lib/target/{profile}/libsample_lib.so"
    ))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Executable Information:");
    let binary: String = get_module_info!(ModuleInfoField::Binary)?;
    let version: String = get_module_info!(ModuleInfoField::Version)?;
    println!("  Binary: {binary}");
    println!("  Version: {version}");

    let lib_path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_lib_path);
    println!(
        "\nLoading shared library via dlopen: {}",
        lib_path.display()
    );

    // The cdylib's `module_info_*` symbols are local to the .so and are
    // NOT exported in its dynamic symbol table. Code inside the library
    // therefore reads the library's own `.note.package` data, even though
    // this executable embeds its own `.note.package` too.
    unsafe {
        let lib = libloading::Library::new(&lib_path)?;
        let print_info: libloading::Symbol<extern "C" fn()> =
            lib.get(b"sample_lib_print_info\0")?;
        let binary_name: libloading::Symbol<extern "C" fn() -> *const c_char> =
            lib.get(b"sample_lib_binary_name\0")?;

        println!("\nLibrary view (read from inside libsample_lib.so):");
        print_info();

        let returned = CStr::from_ptr(binary_name()).to_string_lossy();
        println!("\n  sample_lib_binary_name() returned: {returned}");
        assert_ne!(
            returned, binary,
            "library should report its own binary name, not the executable's"
        );
    }

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
