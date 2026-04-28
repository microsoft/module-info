// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Integration test: verifies that the `.note.package` section is actually
//! present in the built example binary and contains the expected metadata.
//!
//! This catches regressions that the in-process `get_module_info!` call can't:
//! for example, if the linker script were generated but not applied, the
//! runtime API would still pull symbol values from stale memory or fail to
//! resolve them, whereas this test inspects the ELF directly.
//!
//! Linux-only: the note section is only emitted on Linux targets, and
//! `readelf`/`strings` are Linux tooling. Skipped gracefully on other
//! platforms or when `readelf` isn't on PATH (avoids spurious CI failures
//! on slimmed-down build images).

#![cfg(target_os = "linux")]

use std::process::Command;

fn readelf_available() -> bool {
    Command::new("readelf")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn note_package_section_is_present_and_contains_metadata() {
    if !readelf_available() {
        eprintln!("readelf not on PATH; skipping ELF-level note section check");
        return;
    }

    // `CARGO_BIN_EXE_<name>` is set by cargo for integration tests and points
    // at the freshly built binary.
    let exe = env!("CARGO_BIN_EXE_sample_elf_bin");

    // `readelf -n` dumps note sections in a human-readable form including
    // the owner ("FDO") and the JSON body bytes.
    let output = Command::new("readelf")
        .args(["-n", exe])
        .output()
        .expect("readelf failed to spawn");
    assert!(
        output.status.success(),
        "readelf -n exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let notes = String::from_utf8_lossy(&output.stdout);
    assert!(
        notes.contains(".note.package"),
        ".note.package section missing from readelf -n output:\n{notes}"
    );
    assert!(
        notes.contains("FDO"),
        "FDO note owner missing from readelf -n output:\n{notes}"
    );

    // `strings` the binary to confirm the embedded JSON carries the crate
    // name. `readelf -n` prints hex for unknown note types so we fall back
    // to `strings`, which scans the raw bytes.
    let strings = Command::new("strings")
        .arg(exe)
        .output()
        .expect("strings failed to spawn");
    assert!(strings.status.success(), "strings command failed");
    let haystack = String::from_utf8_lossy(&strings.stdout);
    assert!(
        haystack.contains("\"binary\":\"sample_elf_bin\""),
        "embedded JSON did not contain the expected \"binary\" key/value"
    );
}
