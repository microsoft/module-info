// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Verifies that the cdylib's `.note.package` section is `SHT_NOTE`
//! typed, not `SHT_PROGBITS`, and that the embedded JSON carries
//! sample_lib's own `binary` field rather than something inherited from
//! the test runner. The runtime self-read tests in `src/lib.rs` only
//! confirm the bytes are reachable by symbol; they pass even when the
//! section type has degraded to PROGBITS, which is the failure mode
//! that silently breaks `systemd-coredump` and `readelf -n` consumers.
//!
//! The cdylib path is the most regression-prone of the three (rlib /
//! staticlib / cdylib): the `module-info` rlib is only kept in the
//! cdylib's final link because *something* in `sample_lib`'s sources
//! references it (the `get_module_info!` extern statics, `embed!()`'s
//! `#[used] static`, etc.). Drop both references in a refactor and the
//! rlib's `PACKAGE_NOTE_SECTION` input section disappears from the
//! link, leaving ld to synthesize the output section from `BYTE(...)`
//! directives alone (PROGBITS).
//!
//! Linux-only: SHT_NOTE is an ELF concept and the parser the test
//! depends on is gated on little-endian Linux targets.

#![cfg(target_os = "linux")]

// Shared std-only ELF parser. See `examples/_test_support/note_section.rs`
// for why we don't shell out to readelf.
#[path = "../../_test_support/note_section.rs"]
mod note_section;

use note_section::{read_note_package, SHT_NOTE};

use std::env;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn libsample_lib_so_note_package_is_sht_note() {
    // `cargo test` on a crate that is *only* a `cdylib` (no `[[bin]]`,
    // no `rlib` crate-type for tests to link) does not produce
    // `libsample_lib.so` as a side effect of building the test runner;
    // cargo only builds the cdylib when an explicit `cargo build` runs.
    // Build it on demand here so `cargo test` from a clean checkout
    // exercises the section-type guard. Pick `debug` vs `release` to
    // match the test binary's own profile.
    let manifest_dir =
        env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo for tests");
    let manifest_path = PathBuf::from(&manifest_dir).join("Cargo.toml");
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let so_path = PathBuf::from(&manifest_dir)
        .join("target")
        .join(profile)
        .join("libsample_lib.so");

    if !so_path.exists() {
        let mut cmd = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
        cmd.arg("build")
            .arg("--manifest-path")
            .arg(&manifest_path)
            .arg("--quiet");
        if !cfg!(debug_assertions) {
            cmd.arg("--release");
        }
        let status = cmd
            .status()
            .expect("failed to spawn `cargo build` for sample_lib");
        assert!(
            status.success(),
            "cargo build of sample_lib failed (status={status:?})"
        );
    }

    assert!(
        so_path.exists(),
        "expected {} to exist after cargo build of the cdylib",
        so_path.display()
    );

    let note = match read_note_package(&so_path) {
        Ok(n) => n,
        Err(e) => panic!(
            "reading .note.package from {} failed: {e}",
            so_path.display()
        ),
    };

    assert_eq!(
        note.sh_type,
        SHT_NOTE,
        "{}'s .note.package is typed {} (expected SHT_NOTE = {SHT_NOTE}); \
         the rlib's PACKAGE_NOTE_SECTION input section was probably \
         dropped from the cdylib link",
        so_path.display(),
        note.sh_type,
    );

    assert_eq!(
        note.owner,
        "FDO",
        "{}'s .note.package owner is {:?}; FDO spec requires \"FDO\"",
        so_path.display(),
        note.owner,
    );
    assert_eq!(
        note.n_type,
        0xcafe1a7e,
        "{}'s .note.package n_type is {:#x}; expected 0xcafe1a7e",
        so_path.display(),
        note.n_type,
    );

    let descriptor = match note.descriptor_as_str() {
        Ok(s) => s,
        Err(e) => panic!(
            "{}'s .note.package descriptor is not valid UTF-8 ({e})",
            so_path.display()
        ),
    };
    assert!(
        descriptor.contains("\"binary\":\"sample_lib\""),
        "{}'s .note.package did not carry \"binary\":\"sample_lib\":\n{descriptor}",
        so_path.display()
    );
}
