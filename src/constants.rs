// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

/// ELF note section alignment per the System V gABI (§5.2 "Note Section").
///
/// Each note's `name` and `desc` fields are padded up to this boundary. GNU
/// binutils defaults to 4 bytes on Linux for all supported architectures
/// (Solaris/HP-UX use 8-byte note alignment, but those platforms are out of
/// scope for this crate, which is `cfg(target_os = "linux")`-gated).
pub const NOTE_ALIGN: usize = 4;

/// Custom type identifier for our ELF note section
pub const N_TYPE: u32 = 0xcafe1a7e;

/// Owner name for the ELF note section - "FDO" follows Linux standards
pub const OWNER: &str = "FDO";

/// Name of the ELF note section where module info will be stored
pub const NOTE_SECTION_NAME: &str = ".note.package";

/// JSON size limit for the module info section, in bytes.
///
/// Acts as a safeguard against excessively large JSON payloads being stored in
/// the ELF binary's `.note.package` section. 1 KiB is ample for typical module
/// metadata (binary name, versions, git info, OS info, etc.). The limit is
/// enforced by the build-time JSON generator.
pub const MAX_JSON_SIZE: usize = 1024;

/// JSON keys that must be present (and non-empty) in the embedded metadata
/// for the note section to be considered well-formed. Enforced by
/// `validate_embedded_json` at build time so a missing key fails the build
/// rather than the consumer's runtime `get_module_info!` call.
///
/// This is the minimum *identity-plus-platform* set:
/// binary/version/moduleVersion/name uniquely pin the build, `maintainer`
/// ensures a human contact is always embedded, and `os`/`osVersion`
/// identify the platform the artifact was built for (crash-triage tools
/// rely on this to pick the right symbol server / debugger toolchain).
/// The remaining fields defined on `PackageMetadata` (repo, branch, hash,
/// type, copyright) are optional. If the consumer leaves the field empty
/// in their `PackageMetadata`, the embedded JSON ships with `""` for that
/// key and validation passes. See the "Disabling fields" section in the
/// README for the pattern.
pub const REQUIRED_JSON_KEYS: &[&str] = &[
    "binary",
    "version",
    "moduleVersion",
    "name",
    "maintainer",
    "os",
    "osVersion",
];
