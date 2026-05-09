// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Integration test: verifies that the `.note.package` section is actually
//! present in the built example binary, is `SHT_NOTE`-typed (not
//! `SHT_PROGBITS`, which would silently make crash-triage tools ignore
//! it), and that the embedded JSON carries the expected `binary` field.
//!
//! This catches regressions that the in-process `get_module_info!` call
//! can't: for example, if the rlib's `PACKAGE_NOTE_SECTION` static were
//! dropped from the final link, ld would synthesize the section from
//! `BYTE(...)` directives alone and type it `SHT_PROGBITS`. The bytes
//! would still be there, the runtime API would still resolve, but
//! `readelf -n`, `coredumpctl`, and `systemd-coredump` would all silently
//! skip the section. Asserting `sh_type == SHT_NOTE` is the only check
//! that surfaces that regression.
//!
//! Linux-only: `.note.package` is only emitted on Linux targets and the
//! ELF parser the test depends on is gated on little-endian Linux.

#![cfg(target_os = "linux")]

// Shared std-only ELF parser. Lives outside of any single example crate
// so the same logic backs both `sample_elf_bin` and `sample_lib` without
// pulling in a `dev-dependencies` test-helper crate.
#[path = "../../_test_support/note_section.rs"]
mod note_section;

use note_section::{read_note_package, SHT_NOTE};

#[test]
fn note_package_section_is_sht_note_and_carries_binary_field() {
    // `CARGO_BIN_EXE_<name>` is set by cargo for integration tests and
    // points at the freshly built binary, regardless of debug/release.
    let exe = env!("CARGO_BIN_EXE_sample_elf_bin");

    let note = match read_note_package(std::path::Path::new(exe)) {
        Ok(n) => n,
        Err(e) => panic!("reading .note.package failed: {e}"),
    };

    // Section type guard. Distinguishes the two failure modes so a
    // future bisector can tell at a glance whether the section is
    // missing entirely or merely degraded to PROGBITS.
    assert_eq!(
        note.sh_type, SHT_NOTE,
        ".note.package is typed {} (expected SHT_NOTE = {SHT_NOTE}); \
         crash-triage tools filter by section type and would silently \
         ignore this binary's metadata",
        note.sh_type
    );

    // Note-owner guard. The systemd package-metadata format requires the
    // owner string to be "FDO"; consumers (`coredumpctl`, the
    // FDO-decoding `readelf -n` in binutils ≥ 2.39) match on exactly
    // that name and silently ignore notes with any other owner.
    assert_eq!(
        note.owner, "FDO",
        ".note.package owner is {:?}; the FDO package-metadata spec requires \"FDO\" \
         (otherwise crash-triage tools can't recognize the note)",
        note.owner,
    );

    // Vendor-type guard. `0xcafe1a7e` is the crate's `N_TYPE` constant;
    // the systemd FDO Packaging Metadata note is keyed on this exact
    // value. A drift here means the note still parses but FDO-aware
    // tools won't decode the descriptor as JSON metadata.
    assert_eq!(
        note.n_type, 0xcafe1a7e,
        ".note.package n_type is {:#x}; expected the FDO Packaging Metadata constant 0xcafe1a7e",
        note.n_type,
    );

    // Content guard. The descriptor is the embedded JSON; the crate
    // sanitizes it to ASCII at build time so UTF-8 decoding never fails
    // on a well-formed binary.
    let descriptor = match note.descriptor_as_str() {
        Ok(s) => s,
        Err(e) => panic!(
            ".note.package descriptor is not valid UTF-8 ({e}); \
             the crate sanitizes metadata to ASCII so the section was probably corrupted"
        ),
    };
    assert!(
        descriptor.contains("\"binary\":\"sample_elf_bin\""),
        "embedded JSON did not contain \"binary\":\"sample_elf_bin\":\n{descriptor}"
    );
}
