// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Integration test for the cdylib self-read claim.
//!
//! `sample_elf_bin_with_lib` exists to demonstrate that a `cdylib` loaded via
//! `dlopen` reads its *own* `.note.package` data, not the host executable's,
//! even when the host also embeds the section. The unit test next to
//! `main.rs` only checks the executable's own metadata; without this file,
//! `cargo test` would never exercise the dlopen path that the example was
//! built to showcase. A regression in cdylib symbol resolution (linker
//! script not applied to the `.so`, symbols accidentally exported into the
//! `.so`'s dynamic table, host executable's `module_info_*` symbols
//! shadowing the library's, etc.) would silently pass CI without this test.
//!
//! The test:
//! 1. Builds `examples/sample_lib` into a `cdylib` using cargo.
//! 2. Loads the resulting `.so` via `libloading`.
//! 3. Calls `sample_lib_binary_name()` and asserts the returned C string is
//!    `"sample_lib"`, **not** the loader binary's name.
//!
//! Linux-only: `.note.package` embedding is Linux-only, the export shape is
//! ELF-specific, and `dlopen`/`libloading` semantics differ on macOS/Windows.

#![cfg(target_os = "linux")]

use std::env;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::path::PathBuf;
use std::process::Command;

/// Build `examples/sample_lib` and return the path to its `cdylib` artifact.
///
/// Each example under `examples/` is a standalone Cargo package, so we
/// shell out to `cargo build` rather than depending on a shared workspace.
/// The `--quiet` flag suppresses the build-script `cargo:warning=` noise
/// `module-info` emits at compile time so the test output stays focused.
fn build_sample_lib() -> PathBuf {
    // `CARGO_MANIFEST_DIR` is set by cargo for integration tests and points
    // at this example crate's directory, so `../sample_lib` resolves
    // regardless of where the test is invoked from.
    let manifest_dir =
        env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo for tests");
    let sample_lib_manifest = PathBuf::from(&manifest_dir)
        .join("..")
        .join("sample_lib")
        .join("Cargo.toml");

    // Pick the same profile the test binary was built with: `cargo test`
    // defaults to debug, but `cargo test --release` would mismatch.
    let profile_arg = if cfg!(debug_assertions) {
        // Empty: cargo's default profile is debug.
        None
    } else {
        Some("--release")
    };

    let mut cmd = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    cmd.arg("build")
        .arg("--manifest-path")
        .arg(&sample_lib_manifest)
        .arg("--quiet");
    if let Some(p) = profile_arg {
        cmd.arg(p);
    }
    let status = cmd
        .status()
        .expect("failed to spawn `cargo build` for sample_lib");
    assert!(
        status.success(),
        "cargo build of sample_lib failed (status={status:?})"
    );

    let profile_dir = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let so_path = PathBuf::from(&manifest_dir)
        .join("..")
        .join("sample_lib")
        .join("target")
        .join(profile_dir)
        .join("libsample_lib.so");
    assert!(
        so_path.exists(),
        "expected libsample_lib.so at {} after cargo build",
        so_path.display()
    );
    so_path
}

#[test]
fn cdylib_dlopen_reads_its_own_note_package() {
    let so_path = build_sample_lib();

    // SAFETY: the `.so` is one this test just built; its exports
    // (`sample_lib_binary_name`, `sample_lib_print_info`) are declared
    // `extern "C"` in `examples/sample_lib/src/lib.rs`. The library lives
    // for the duration of this test scope; pointers it returns are valid
    // until then.
    unsafe {
        let lib = libloading::Library::new(so_path).expect("dlopen of libsample_lib.so failed");

        let binary_name: libloading::Symbol<extern "C" fn() -> *const c_char> = lib
            .get(b"sample_lib_binary_name\0")
            .expect("sample_lib_binary_name export missing");

        let returned_ptr = binary_name();
        assert!(
            !returned_ptr.is_null(),
            "sample_lib_binary_name returned a null pointer"
        );
        let returned = CStr::from_ptr(returned_ptr).to_string_lossy().to_string();

        // Self-read invariant: the cdylib's exported accessor must report
        // the library's own embedded `binary` field, not the host
        // executable's. The loader binary embeds
        // `binary = "sample_elf_bin_with_lib"`, so any answer other than
        // `"sample_lib"` indicates the cdylib is reading metadata from the
        // wrong ELF module (linker script not applied, dynamic-table
        // collision, etc.).
        assert_eq!(
            returned, "sample_lib",
            "cdylib should report its own `binary` field; got {returned:?}"
        );
    }
}
