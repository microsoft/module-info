// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Embed metadata into ELF binaries as `.note.package` sections so it
//! **survives crashes**, visible to `coredumpctl`, `readelf -n`, and any
//! other consumer of the [systemd package-metadata
//! format](https://uapi-group.org/specifications/specs/package_metadata_for_executable_files/).
//! The crate's main feature is crash-dump preservation: when your process dies,
//! the version of code that crashed is recoverable from the core dump without external
//! symbol files or build-system context.
//!
//! Runtime read-back via the [`get_module_info!`] macro is a *convenience
//! accessor*, useful while the process is still alive but not the reason
//! the crate exists.
//!
//! Consumers call [`generate_project_metadata_and_linker_script`] from
//! `build.rs` to generate the linker script and Cargo directives. At
//! runtime, metadata fields can be read via [`get_module_info!`] (returns
//! `ModuleInfoResult<String>` for a single field, or a `HashMap` of all
//! readable fields when called with no arguments). On non-Linux platforms
//! the crate exposes no-op stubs so cross-platform builds still compile;
//! runtime accessors return `ModuleInfoError::NotAvailable`.
//!
//! See the README and the `examples/` directory for an end-to-end integration.
//!
//! # Limitations
//!
//! **Shared libraries (`cdylib`/`dylib`) cannot reliably read their own metadata.**
//! The linker-script symbols that [`get_module_info!`] resolves
//! (`module_info_binary`, `module_info_version`, …) are not namespaced per
//! crate. When a `cdylib` that embeds its own `.note.package` is loaded by an
//! executable that also embeds one, the dynamic linker resolves both sides'
//! references to a single definition, typically the main executable's copy.
//! The library will read the executable's metadata, not its own. Treat the
//! runtime API as binary-only; for shared libraries, parse the ELF note
//! section directly from the library file instead.
//!
//! **Little-endian targets only.** The ELF note header is serialized with
//! `u32::to_le_bytes` at `build.rs` time. Supported targets today are
//! `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, and
//! `i686-unknown-linux-gnu` (all little-endian). Cross-compiling for a
//! big-endian Linux target (s390x, powerpc-be, mips-be) will silently emit
//! a byte-swapped note section that `readelf -n` and `systemd-coredump`
//! cannot parse. Adding big-endian support would mean selecting `to_le_bytes`
//! vs `to_be_bytes` from `CARGO_CFG_TARGET_ENDIAN`.

mod error;
mod fields;
// `#[macro_use]` makes the non-exported build-time helpers (note!/error!/
// warn!/debug!) visible to sibling modules without exporting them.
#[macro_use]
mod macros;
use cfg_if::cfg_if;
pub use error::{ModuleInfoError, ModuleInfoResult};
pub use fields::ModuleInfoField;

cfg_if! {
    if #[cfg(target_os = "linux")] {
        use std::{env, path::{Path, PathBuf}};

        mod constants;
        mod metadata;
        mod note_section;
        mod utils;

        pub use metadata::PackageMetadata;

        pub(crate) use constants::*;
    }
}

cfg_if! {
    if #[cfg(all(feature = "embed-module-info", target_os = "linux"))] {
        /// Static symbol that marks the beginning of our custom note section
        ///
        /// This empty array is placed in the .note.package section and serves as an anchor
        /// for the linker script to place our metadata properly.
        #[link_section = ".note.package"]
        #[no_mangle]
        #[used]
        #[doc(hidden)]
        pub static PACKAGE_NOTE_SECTION: [u8; 0] = [];

        /// Force the `module_info` rlib to be linked into the consuming binary so the
        /// `.note.package` section is emitted with ELF type `SHT_NOTE`.
        ///
        /// # Why this is needed
        ///
        /// The note data is produced by the linker script that `build.rs` generates.
        /// Without a source-level reference to this crate, however, cargo/rustc drops
        /// the `module_info` rlib from the final link, and GNU ld then cannot inherit
        /// the `SHT_NOTE` type from an input section. The output `.note.package`
        /// becomes `SHT_PROGBITS` instead. The bytes are present, but tools like
        /// `readelf -n` and `systemd-coredump` filter by section type and ignore it.
        ///
        /// Invoking `module_info::embed!()` at the crate root creates a `#[used]`
        /// reference to [`PACKAGE_NOTE_SECTION`], which forces the rlib to link and
        /// restores the correct section type.
        ///
        /// # When to use it
        ///
        /// Use `embed!()` when the consuming crate does **not** call `get_module_info!`
        /// or reference any other `module_info` item at runtime (pure build-time
        /// embedding). When the consuming crate already calls
        /// `module_info::get_module_info!(...)` or imports any item from the crate,
        /// this macro is unnecessary; the rlib is already linked.
        ///
        /// # Example
        ///
        /// ```ignore
        /// // Top of src/main.rs or src/lib.rs:
        /// module_info::embed!();
        ///
        /// fn main() {
        ///     // No other module_info references needed for the .note.package
        ///     // section to end up in the binary with SHT_NOTE type.
        /// }
        /// ```
        #[macro_export]
        macro_rules! embed {
            () => {
                #[allow(dead_code)]
                const _: () = {
                    #[used]
                    static __MODULE_INFO_FORCE_LINK: &'static [u8; 0] =
                        &$crate::PACKAGE_NOTE_SECTION;
                };
            };
        }
    } else if #[cfg(all(feature = "embed-module-info", not(target_os = "linux")))] {
        /// No-op stub of [`embed!`](embed) for non-Linux targets. Present so
        /// cross-platform builds compile without `#[cfg]` guards at each call site.
        #[macro_export]
        macro_rules! embed {
            () => {};
        }
    } else {
        /// No-op stub of [`embed!`](embed) for feature-off builds (the
        /// `embed-module-info` feature is disabled). Present so a consumer that
        /// uses `module_info` only for `get_version()` / `get_module_version()`
        /// can still call `module_info::embed!()` in their crate root without a
        /// feature-gated `#[cfg]` guard. The macro expands to nothing because
        /// there is no note section to anchor when the feature is off.
        #[macro_export]
        macro_rules! embed {
            () => {};
        }
    }
}

/// Options controlling how [`embed_package_metadata`] writes artifacts and
/// whether it emits cargo link-arg directives.
///
/// `EmbedOptions::default()` preserves the original zero-config behavior:
/// write to `$OUT_DIR` and emit `cargo:rustc-link-arg=-T<linker_script.ld>`.
/// Override when the crate is a static library whose final link happens later
/// in the outer build system.
///
/// # Non-exhaustive
///
/// This struct is `#[non_exhaustive]` so new options can land without a
/// SemVer break. Use `..Default::default()` when constructing.
///
/// # Example
/// ```rust,no_run
/// # use module_info::EmbedOptions;
/// // Static-library flow: write the linker script to a directory the outer
/// // build system knows about, so it can pass the script to the final linker.
/// // In practice `out_dir` comes from an env var the outer build sets, or a
/// // subdirectory of `OUT_DIR`; here we use `env::temp_dir()` as a portable
/// // placeholder. `EmbedOptions` is `#[non_exhaustive]`, so construct via
/// // `Default` and assign fields rather than using struct-literal syntax.
/// let mut opts = EmbedOptions::default();
/// opts.out_dir = Some(std::env::temp_dir().join("module_info_linker"));
/// opts.emit_cargo_link_arg = false;
/// ```
#[cfg(target_os = "linux")]
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct EmbedOptions {
    /// Directory where `linker_script.ld`, `note.package.bin`, and
    /// `module_info.json` are written. When `None`, the `OUT_DIR` environment
    /// variable is used (the normal cargo build-script case).
    pub out_dir: Option<PathBuf>,

    /// When `true`, emit `cargo:rustc-link-arg=-T<path-to-linker_script.ld>`
    /// on stdout so cargo passes the script to the final link step.
    ///
    /// Set to `false` when the current crate is a static library whose final
    /// link happens later in the outer build system. Have that system pass
    /// the linker script to its own linker.
    pub emit_cargo_link_arg: bool,
}

#[cfg(target_os = "linux")]
impl Default for EmbedOptions {
    fn default() -> Self {
        Self {
            out_dir: None,
            emit_cargo_link_arg: true,
        }
    }
}

/// Artifacts written by [`embed_package_metadata`].
///
/// Returned so consumers can log, inspect, or pass paths to a later build
/// step (for the static-library flow, typically `linker_script_path`).
///
/// # Non-exhaustive
///
/// `#[non_exhaustive]`. Constructed by the crate, not by consumers.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct EmbedArtifacts {
    /// Absolute path to the generated linker script (`linker_script.ld`).
    pub linker_script_path: PathBuf,
    /// Absolute path to the raw `.note.package` binary dump.
    pub note_bin_path: PathBuf,
    /// Absolute path to the compact JSON metadata (`module_info.json`).
    pub json_path: PathBuf,
    /// JSON string written to `module_info.json` and embedded as the note
    /// section's descriptor. One key:value pair per line (not strictly
    /// "compact"); the runtime scan in `extract_module_info` tolerates the
    /// embedded newlines.
    pub json: String,
    /// Byte-encoded linker script body that produced `linker_script.ld`.
    pub linker_script_body: String,
}

/// Convenience struct-literal view over [`PackageMetadata`] with field names
/// shaped like the JSON keys rather than the internal Rust snake_case names.
///
/// `Info` exists so call sites can read the same way the embedded JSON reads:
/// `r#type`, `moduleVersion`, `osVersion` instead of `module_type`,
/// `module_version`, `os_version`. It's deliberately **not** `#[non_exhaustive]`:
/// struct-literal construction is the whole point. Pass it to [`new`] to build
/// the note artifacts in one call:
///
/// # Forward compatibility
///
/// **Always terminate the struct literal with `..Default::default()`.** Unlike
/// [`PackageMetadata`] (which is `#[non_exhaustive]` and forbids struct-literal
/// construction from outside the crate, forcing consumers into the
/// field-assignment pattern that is intrinsically forward-compatible), `Info`
/// permits a fully-exhaustive literal. That means a minor release of this
/// crate that adds a new field will break any `Info { … }` call site that
/// listed every field by name. The `..Default::default()` terminator is how
/// consumers buy forward compatibility: new fields fall back to their
/// `Default` value (empty string / disabled) instead of failing to compile.
/// This is the *only* reason `Info` is safe to add fields to in minor
/// releases. Omit the terminator and the crate can no longer do that
/// without breaking you.
///
/// ```rust,no_run
/// # use module_info::Info;
/// let _ = module_info::new(Info {
///     binary: "my_tool".into(),
///     name: "my_tool".into(),
///     maintainer: "team@contoso.com".into(),
///     version: "1.2.3".into(),
///     moduleVersion: "1.2.3.4".into(),
///     os: "linux".into(),
///     osVersion: "22.04".into(),
///     r#type: "agent".into(),
///     hash: "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef".into(),
///     ..Default::default()
/// });
/// ```
///
/// Under the hood `new` converts this to a [`PackageMetadata`] and calls
/// [`embed_package_metadata`] with [`EmbedOptions::default()`].
///
/// # No auto-detection on this path
///
/// Every field in the `Info` literal ships verbatim. `os`/`osVersion` are
/// **not** read from `/etc/os-release`, and `repo`/`branch`/`hash` are
/// **not** read from git. The caller owns every value. If you want the
/// `/etc/os-release` + git auto-detection that the zero-config entry point
/// provides, reach for [`PackageMetadata::from_cargo_toml`] instead,
/// mutate the fields you want to override, and pass the result to
/// [`embed_package_metadata`].
///
/// # Disabling fields
///
/// Seven keys are required at validation time:
/// `binary`, `version`, `moduleVersion`, `name`, `maintainer`, `os`, and
/// `osVersion`. The rest (`r#type`, `repo`, `branch`, `hash`, `copyright`)
/// may be left as the empty string (the `Default` value);
/// `..Default::default()` in the literal above is the idiomatic way to
/// opt out. The embedded JSON still carries every key (the
/// `.note.package` layout is fixed), but the value ships as `""`, which
/// downstream tooling can treat as "disabled."
///
/// # `r#type` tradeoff
///
/// The JSON key is `type`, which collides with Rust's `type` keyword. We use
/// the raw-identifier form `r#type` rather than a `#[serde(rename = "type")]`
/// alias on a differently-named field (say, `module_type`), because the
/// latter would require call sites to remember the rename when constructing
/// the struct literal, re-creating the original mismatch this type is meant
/// to solve. `r#type` is ugly but pays off once: downstream construction
/// reads `r#type: "agent".into()` and the JSON reads `"type":"agent"`.
#[cfg(target_os = "linux")]
#[allow(non_snake_case)] // JSON-key-shaped field names (moduleVersion, osVersion) are intentional.
#[derive(Debug, Clone, Default)]
pub struct Info {
    /// Binary name (matches JSON key `binary`).
    pub binary: String,
    /// Crate version from Cargo.toml (matches JSON key `version`).
    pub version: String,
    /// Full 4-part module version (matches JSON key `moduleVersion`).
    pub moduleVersion: String,
    /// Maintainer contact information (matches JSON key `maintainer`).
    pub maintainer: String,
    /// Package name (matches JSON key `name`).
    pub name: String,
    /// Module type: agent, library, executable, etc. (matches JSON key `type`).
    pub r#type: String,
    /// Git repository name (matches JSON key `repo`).
    pub repo: String,
    /// Git branch name (matches JSON key `branch`).
    pub branch: String,
    /// Git commit hash (matches JSON key `hash`).
    pub hash: String,
    /// Copyright information (matches JSON key `copyright`).
    pub copyright: String,
    /// Operating system name (matches JSON key `os`).
    pub os: String,
    /// Operating system version (matches JSON key `osVersion`).
    pub osVersion: String,
}

#[cfg(target_os = "linux")]
impl From<Info> for PackageMetadata {
    fn from(info: Info) -> Self {
        PackageMetadata {
            binary: info.binary,
            version: info.version,
            module_version: info.moduleVersion,
            maintainer: info.maintainer,
            name: info.name,
            module_type: info.r#type,
            repo: info.repo,
            branch: info.branch,
            hash: info.hash,
            copyright: info.copyright,
            os: info.os,
            os_version: info.osVersion,
        }
    }
}

/// One-call entry point: convert [`Info`] → [`PackageMetadata`] and embed via
/// [`embed_package_metadata`] with [`EmbedOptions::default()`].
///
/// Use this from `build.rs` when you want to supply metadata programmatically
/// without touching `Cargo.toml` and don't need to override any
/// [`EmbedOptions`] (custom `out_dir`, suppressed `cargo:rustc-link-arg`, …).
/// For those cases, convert `Info` to `PackageMetadata` with `.into()` and
/// call [`embed_package_metadata`] directly.
///
/// # Errors
/// Propagates everything [`embed_package_metadata`] can return, plus
/// `ModuleInfoError::MalformedJson` if `moduleVersion` is not four
/// dot-separated numeric parts that each fit in a `u16`.
#[cfg(target_os = "linux")]
#[must_use = "new returns EmbedArtifacts; discarding it hides both the written paths and any I/O errors"]
pub fn new(info: Info) -> ModuleInfoResult<EmbedArtifacts> {
    embed_package_metadata(&info.into(), &EmbedOptions::default())
}

/// Validate that `module_version` is exactly four dot-separated numeric parts,
/// each of which fits in a `u16` (0..=65535).
///
/// This mirrors the Windows `VS_FIXEDFILEINFO::FILEVERSION` shape (four
/// `WORD`-sized components) that Windows-style crash consumers expect to
/// parse. An out-of-range value silently truncating on the consumer side
/// would be worse than failing the build, so we enforce the range at embed
/// time.
#[cfg(target_os = "linux")]
fn validate_module_version(module_version: &str) -> ModuleInfoResult<()> {
    let parts: Vec<&str> = module_version.split('.').collect();
    if parts.len() != 4 {
        return Err(ModuleInfoError::MalformedJson(format!(
            "moduleVersion must have exactly 4 dot-separated parts, got {} in {module_version:?}",
            parts.len()
        )));
    }
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            return Err(ModuleInfoError::MalformedJson(format!(
                "moduleVersion part {i} is empty in {module_version:?}"
            )));
        }
        if part.parse::<u16>().is_err() {
            return Err(ModuleInfoError::MalformedJson(format!(
                "moduleVersion part {i} ({part:?}) must be a non-negative integer \
                 that fits in 16 bits (0..=65535) in {module_version:?}"
            )));
        }
    }
    Ok(())
}

/// Embed a [`PackageMetadata`] value into ELF note artifacts on disk.
///
/// Consumers that want to supply metadata programmatically (e.g. from
/// `build.rs` without editing `Cargo.toml`) or suppress the
/// `cargo:rustc-link-arg` directive (e.g. a static library whose final link
/// happens in a later build step) call this directly; the zero-config
/// [`generate_project_metadata_and_linker_script`] is a thin wrapper over
/// this function with the default options.
///
/// # Errors
/// Returns `ModuleInfoError::MetadataTooLarge` if the serialized JSON exceeds
/// the 1 KiB `.note.package` payload limit, or `ModuleInfoError::MalformedJson`
/// if a required field is missing. `IoError` on filesystem failures.
#[cfg(target_os = "linux")]
#[must_use = "embed_package_metadata returns EmbedArtifacts; discarding it hides both the written paths and any I/O errors"]
pub fn embed_package_metadata(
    md: &PackageMetadata,
    opts: &EmbedOptions,
) -> ModuleInfoResult<EmbedArtifacts> {
    // Emit rerun directives *before* any failure path. Emitting any `cargo:`
    // directive opts out of cargo's default "rerun on any file change", so
    // without these the build script wouldn't re-run when Cargo.toml, git
    // HEAD, or env vars change. Stamped metadata would silently go stale.
    emit_rerun_if_directives();

    let (compact_json, linker_script_body) = metadata::render_note_payloads(md)?;

    validate_embedded_json(&compact_json)?;

    note!();
    note!("-- Module Info --");
    emit_metadata_notes(&compact_json);

    let out_dir: PathBuf = match &opts.out_dir {
        Some(p) => p.clone(),
        None => PathBuf::from(env::var("OUT_DIR")?),
    };
    debug!("OUT_DIR: {}", out_dir.display());

    std::fs::create_dir_all(&out_dir)?;
    // `.ld.inc` signals include fragment (no SECTIONS/INSERT wrapper;
    // inlined inside linker_script.ld).
    let linker_script_body_path = out_dir.join("linker_script_body.ld.inc");
    debug!(
        "Writing linker script body to: {}",
        linker_script_body_path.display()
    );
    // Header comment + trim of leading blank line prevents the standalone file
    // from looking like a truncated linker script.
    let linker_script_body_on_disk = format!(
        "/* Linker-script fragment. Inlined inside linker_script.ld; not a standalone script. */\n{}",
        linker_script_body.trim_start_matches('\n')
    );
    std::fs::write(
        &linker_script_body_path,
        linker_script_body_on_disk.as_bytes(),
    )?;

    let json_path = out_dir.join("module_info.json");
    debug!("Writing module info to: {}", json_path.display());
    std::fs::write(&json_path, compact_json.as_bytes())?;

    // Descriptor must include the same NUL padding the linker script emits
    // after the JSON (see `render_note_payloads`); otherwise `descsz` covers
    // only JSON bytes while the section includes padding, and `readelf -n`
    // warns "Corrupt note: only N bytes remain".
    let padding = NOTE_ALIGN - (compact_json.len() % NOTE_ALIGN);
    let mut descriptor = String::with_capacity(compact_json.len() + padding);
    descriptor.push_str(&compact_json);
    for _ in 0..padding {
        descriptor.push('\0');
    }

    let note = note_section::NoteSection::new(
        N_TYPE,
        OWNER,
        &descriptor,
        &linker_script_body,
        NOTE_ALIGN,
    )?;
    debug!(
        "Created note section with {} bytes of data",
        note.note_section.len()
    );

    // Strip the leading `.` so the dump isn't a dotfile hidden by default.
    let note_bin_path = out_dir.join(format!("{}.bin", NOTE_SECTION_NAME.trim_start_matches('.')));
    debug!("Saving binary note section to: {}", note_bin_path.display());
    note.save_section(&note_bin_path)?;

    debug!("Saving linker script...");
    let linker_script_path = note.save_linker_script(&out_dir)?;
    debug!("Linker script saved to: {}", linker_script_path.display());

    match link_arg_directive(&linker_script_path, opts.emit_cargo_link_arg) {
        Some(d) => {
            debug!("Adding cargo directive: {}", d);
            println!("{d}");
        }
        None => {
            debug!(
                "emit_cargo_link_arg=false: caller will pass {} to the final linker",
                linker_script_path.display()
            );
        }
    }

    Ok(EmbedArtifacts {
        linker_script_path,
        note_bin_path,
        json_path,
        json: compact_json,
        linker_script_body,
    })
}

/// Validate the serialized metadata JSON: size limit, object shape, required fields.
#[cfg(target_os = "linux")]
fn validate_embedded_json(desc_json: &str) -> ModuleInfoResult<()> {
    if desc_json.len() > constants::MAX_JSON_SIZE {
        return Err(ModuleInfoError::MetadataTooLarge(format!(
            "Metadata size {} exceeds limit of {} bytes",
            desc_json.len(),
            constants::MAX_JSON_SIZE
        )));
    }

    let value: serde_json::Value = serde_json::from_str(desc_json)
        .map_err(|e| ModuleInfoError::MalformedJson(e.to_string()))?;

    if !value.is_object() {
        return Err(ModuleInfoError::MalformedJson(
            "Metadata must be a JSON object".to_string(),
        ));
    }

    for field in constants::REQUIRED_JSON_KEYS {
        // `PackageMetadata` derives `Serialize` with no skip_if, so every key
        // is always present in the JSON; a bare `is_none()` check here would
        // pass a `PackageMetadata::default()` value through untouched. Treat
        // both "missing key" and "empty string value" as missing so a
        // Default-constructed `PackageMetadata` with a forgotten required
        // field fails the build instead of silently embedding `""`.
        let present_and_nonempty = value
            .get(field)
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        if !present_and_nonempty {
            return Err(ModuleInfoError::MalformedJson(format!(
                "Required field '{field}' is missing or empty"
            )));
        }
    }

    // `moduleVersion` is a required key and the loop above has already
    // rejected any payload where it's missing, non-string, or empty. Fetch
    // it unconditionally here; an `if let Some(...) = ...` arm would silently
    // no-op if a future refactor ever split the required-keys check from the
    // presence check, letting malformed payloads slip through a code path
    // that is supposed to be the range guardrail. Using `.ok_or_else` makes
    // the dependency on the loop above load-bearing and visible.
    let mv = value
        .get("moduleVersion")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ModuleInfoError::MalformedJson(
                "moduleVersion must be a non-empty string by this point (required-keys check above enforces it)"
                    .to_string(),
            )
        })?;
    validate_module_version(mv)?;

    Ok(())
}

/// Print metadata key/value pairs as cargo `note!` lines in a stable order.
#[cfg(target_os = "linux")]
fn emit_metadata_notes(desc_json: &str) {
    // Presentation step only; validation already ran. Log via `debug!` so
    // `MODULE_INFO_DEBUG=true` reveals why the notes pane is empty on the
    // impossible case of a parse failure slipping through.
    let map = match serde_json::from_str::<serde_json::Value>(desc_json) {
        Ok(serde_json::Value::Object(map)) => map,
        Ok(other) => {
            debug!("emit_metadata_notes: expected a JSON object, got {}", other);
            return;
        }
        Err(e) => {
            debug!("emit_metadata_notes: JSON parse failed: {}", e);
            return;
        }
    };

    // Walk `ModuleInfoField::ALL` for stable order. No extra-keys fallback
    // needed: `PackageMetadata` is `#[non_exhaustive]`, so the key set is
    // always exactly `ModuleInfoField::ALL`.
    for field in ModuleInfoField::ALL {
        let key = field.to_key();
        if let Some(value) = map.get(key) {
            match value.as_str() {
                Some(s) => note!("{}: {}", key, s),
                None => note!("{}: {}", key, value.to_string()),
            }
        }
    }
}

/// Format the `cargo:rustc-link-arg=-T<path>` directive, or `None` when the
/// caller opted out via `EmbedOptions::emit_cargo_link_arg = false`. Free
/// function so tests can observe the gating without capturing stdout.
#[cfg(target_os = "linux")]
fn link_arg_directive(linker_script_path: &Path, emit: bool) -> Option<String> {
    if emit {
        Some(format!(
            "cargo:rustc-link-arg=-T{}",
            linker_script_path.display()
        ))
    } else {
        None
    }
}

/// Emit `cargo:rerun-if-changed` / `cargo:rerun-if-env-changed` directives
/// covering the inputs this crate reads.
///
/// Cargo's default behavior is "rerun build.rs on any file change in the
/// crate directory." Emitting *any* `cargo:` directive (we emit
/// `rustc-link-arg` further down) flips cargo into explicit-only mode,
/// after which it reruns only when a path/env we name here changes. So we
/// have to list every input, or builds silently reuse stale stamped metadata.
///
/// What we cover:
/// - `Cargo.toml` - `[package]` version/name + `[package.metadata.module_info]`
/// - `build.rs` - the caller's build script itself
/// - `.git/HEAD` + `.git/refs` - so branch switches and new commits retrigger
///   the git-derived fields (`branch`, `hash`, `repo`)
/// - `/etc/os-release` - so a distro upgrade retriggers `os` / `osVersion`
/// - `MODULE_INFO_DEBUG` - the crate's own debug knob
/// - `CARGO_PKG_*` env vars - Cargo sets these from Cargo.toml so they're
///   technically redundant with `rerun-if-changed=Cargo.toml`, but listing
///   them is cheap and removes a foot-gun if a caller ever sets them
///   externally.
///
/// What we *don't* cover: caller-custom env vars (e.g. `BUILD_BUILDNUMBER`
/// named via `[package.metadata.module_info].version_env_var_name`). The
/// zero-config path emits those itself in `collect_package_metadata` because
/// only that path knows the names. Builder-API consumers that read arbitrary
/// env vars must emit their own `cargo:rerun-if-env-changed=<name>` for
/// each, the crate can't guess.
#[cfg(target_os = "linux")]
fn emit_rerun_if_directives() {
    // Paths the crate reads during build. Using forward-slash relative paths
    // makes these valid on all Cargo-supported hosts; the git and
    // os-release watches silently no-op when the path doesn't exist (e.g.
    // building a tarballed source tree, or on a non-Linux host).
    for path in [
        "Cargo.toml",
        "build.rs",
        ".git/HEAD",
        ".git/refs",
        ".git/packed-refs",
        "/etc/os-release",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }

    // Env vars the crate itself reads. Custom ones named in Cargo.toml are
    // handled in `collect_package_metadata`.
    for env_var in ["MODULE_INFO_DEBUG", "CARGO_PKG_NAME", "CARGO_PKG_VERSION"] {
        println!("cargo:rerun-if-env-changed={env_var}");
    }
}

/// Zero-configuration build-script entry point.
///
/// Reads metadata from `Cargo.toml`, env overrides, git, and OS release info,
/// then embeds it via [`embed_package_metadata`] with
/// [`EmbedOptions::default()`]. Reach for [`embed_package_metadata`] directly
/// when you need to supply metadata programmatically or suppress the
/// `cargo:rustc-link-arg` directive.
///
/// # IMPORTANT
/// Only call from `build.rs`. Cargo sets `OUT_DIR` and related variables for
/// build scripts; outside that context the call will fail.
///
/// # Example
/// ```rust,no_run
/// // In build.rs
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     module_info::generate_project_metadata_and_linker_script()?;
///     Ok(())
/// }
/// ```
///
/// # Errors
/// Returns an error if metadata generation or file operations fail.
#[cfg(target_os = "linux")]
#[must_use = "build.rs must propagate errors from this function, otherwise a missing linker script will silently break the ELF note section"]
pub fn generate_project_metadata_and_linker_script() -> Result<(), Box<dyn std::error::Error>> {
    let md = PackageMetadata::from_cargo_toml().map_err(|e| {
        error!("Failed to get project metadata: {}", e);
        e
    })?;
    // Named binding (not `let _`) so `#[must_use]` keeps firing on future
    // signature changes; paths below are useful in build logs.
    let artifacts = embed_package_metadata(&md, &EmbedOptions::default())?;
    debug!(
        "Wrote linker script: {}",
        artifacts.linker_script_path.display()
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
#[must_use = "build.rs must propagate errors from this function, otherwise a missing linker script will silently break the ELF note section"]
pub fn generate_project_metadata_and_linker_script() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

/// Prints all available module info to stdout and returns a result indicating success or failure
///
/// This utility function retrieves all embedded module information and
/// outputs it to the console with labels. It's useful for debugging or displaying
/// version information in command-line tools.
///
/// # Examples
///
/// Basic usage with simple error handling:
/// ```rust,no_run
/// if module_info::print_module_info().is_ok() {
///     println!("Module info displayed successfully");
/// }
/// ```
///
/// Error handling:
/// ```rust,no_run
/// use module_info::{print_module_info, ModuleInfoError};
///
/// match print_module_info() {
///     Ok(_) => println!("Module info displayed successfully"),
///     Err(ModuleInfoError::NotAvailable(msg)) => eprintln!("Module info not available: {}", msg),
///     Err(e) => eprintln!("Failed to display module info: {}", e),
/// }
/// ```
///
/// # Errors
///
/// This function will return an error in the following situations:
/// - If any of the seven required identity-plus-platform fields (`binary`,
///   `version`, `moduleVersion`, `name`, `maintainer`, `os`, `osVersion`) is
///   missing or empty, suggesting the metadata is missing or corrupted
///   (returns `ModuleInfoError::NotAvailable`)
/// - If running on a non-Linux platform where module info isn't supported (returns `ModuleInfoError::NotAvailable`)
///
/// # Note
/// This function is only available when the "embed-module-info" feature is
/// enabled *and* the target OS is Linux. On other platforms the function
/// exists as a no-op stub that returns `NotAvailable`, matching the
/// non-Linux `get_module_info!` macro behavior so cross-platform callers
/// compile unchanged.
#[cfg(all(feature = "embed-module-info", target_os = "linux"))]
#[must_use = "print_module_info returns a Result indicating whether the embedded note section was readable; ignoring it will hide missing-metadata errors"]
pub fn print_module_info() -> ModuleInfoResult<()> {
    // Delegate to the `get_module_info!()` macro: it handles the extern-static
    // declarations, the per-field `extract_module_info` call, platform gating,
    // and error swallowing for individual fields. On non-Linux it returns
    // `NotAvailable` directly, which propagates via `?`.
    let info = get_module_info!()?;

    // Optional fields may legitimately be empty (see README "Disabling fields"),
    // so only required keys are checked here.
    let missing: Vec<&str> = constants::REQUIRED_JSON_KEYS
        .iter()
        .filter(|key| info.get(**key).map_or(true, |v| v.is_empty()))
        .copied()
        .collect();
    if !missing.is_empty() {
        return Err(ModuleInfoError::NotAvailable(format!(
            "Module info appears to be missing or corrupted: required field(s) missing or empty: {}",
            missing.join(", ")
        )));
    }

    for field in ModuleInfoField::ALL {
        let key = field.to_key();
        match info.get(key) {
            Some(value) => println!("{key}: {value}"),
            None => println!("{key}: <unavailable>"),
        }
    }
    Ok(())
}

/// Non-Linux stub: the embedded note section only exists on Linux, so there's
/// nothing to read. Returns `NotAvailable` with a platform-specific message,
/// matching the non-Linux `get_module_info!` macro so cross-platform callers
/// don't need their own `#[cfg]` gate.
#[cfg(any(not(feature = "embed-module-info"), not(target_os = "linux")))]
#[must_use = "print_module_info returns a Result indicating whether the embedded note section was readable; ignoring it will hide missing-metadata errors"]
pub fn print_module_info() -> ModuleInfoResult<()> {
    Err(ModuleInfoError::NotAvailable(
        "Module info is only available on Linux platforms with the embed-module-info feature enabled.".to_string(),
    ))
}

/// Returns the embedded `version` field (from `Cargo.toml`'s `package.version`
/// or `version_env_var_name`) as a `String`.
///
/// Thin wrapper around `get_module_info!(ModuleInfoField::Version)`. See the
/// crate-level "Limitations" section for shared-library symbol-resolution
/// caveats.
///
/// # Errors
///
/// Returns `ModuleInfoError::NotAvailable` on non-Linux targets or when the
/// `embed-module-info` feature is not enabled. Returns `NullPointer`,
/// `Utf8Error`, or `MalformedJson` if the note section is missing or corrupt.
#[cfg(feature = "embed-module-info")]
#[must_use = "get_version returns the embedded version string; discarding it hides missing-metadata errors"]
pub fn get_version() -> ModuleInfoResult<String> {
    get_module_info!(ModuleInfoField::Version)
}

/// Returns the embedded `moduleVersion` field (a 4-part identifier typically
/// produced by the build pipeline; see `module_version_env_var_name` in
/// `Cargo.toml`'s `[package.metadata.module_info]`).
///
/// See [`get_version`] for symbol-resolution and error semantics.
#[cfg(feature = "embed-module-info")]
#[must_use = "get_module_version returns the embedded 4-part module version; discarding it hides missing-metadata errors"]
pub fn get_module_version() -> ModuleInfoResult<String> {
    get_module_info!(ModuleInfoField::ModuleVersion)
}

/// Extract a single module info field from a linker script symbol
///
/// This function extracts a string value from a raw pointer that is expected to point to a
/// null-terminated C string containing a JSON string value. It parses the embedded metadata
/// from the note section of an executable or shared library file.
///
/// # Safety
/// This function is unsafe because it:
/// - Takes a raw pointer that must be a valid pointer to a null-terminated C string
/// - Will cause undefined behavior if the pointer is invalid, dangling, or points to memory that is not properly null-terminated
/// - Dereferencing invalid pointers can lead to memory corruption, segfaults, or security vulnerabilities
/// - Expects the string to contain a JSON string value (surrounded by quotes) and will misbehave otherwise
///
/// # Important
/// This function is **NOT** intended for direct use by consumers of this library.
/// Always use the `get_module_info!` macro instead, which provides proper safety guarantees.
///
/// # Requirements for Safe Usage
/// Callers must guarantee that:
/// 1. The pointer is properly aligned and points to valid memory
/// 2. The memory contains a valid null-terminated C string
/// 3. The memory will remain valid for the duration of this function call
/// 4. Only module info symbols that are statically allocated should be passed to this function
///
/// The `get_module_info!` macro handles all these requirements correctly by:
/// - Only accepting static symbol identifiers declared with proper types (never arbitrary pointers)
/// - Ensuring type safety through Rust's macro system
/// - Maintaining a controlled set of symbols that can be passed to this function
///
/// # Example
/// ```rust,no_run
/// // Direct usage with explicit imports
/// use module_info::{get_module_info, ModuleInfoField, ModuleInfoResult};
///
/// // Correct usage through the macro:
/// let binary_info: ModuleInfoResult<String> = get_module_info!(ModuleInfoField::Binary);
///
/// // Direct usage is unsafe and not recommended:
/// // unsafe { extract_module_info(ptr) } // DON'T DO THIS
/// ```
///
/// # Errors
/// Returns specific error variants through the `ModuleInfoError` enum:
/// - `ModuleInfoError::NullPointer` - If the pointer is null
/// - `ModuleInfoError::Utf8Error` - If the string cannot be parsed as valid UTF-8
/// - `ModuleInfoError::MalformedJson` - If the expected JSON string format is not found
/// - `ModuleInfoError::NotAvailable` - If module info is not available on this platform
///
/// # Note
/// This function is only available when the "embed-module-info" feature is enabled on Linux platforms.
#[cfg(all(feature = "embed-module-info", target_os = "linux"))]
#[must_use = "extract_module_info returns the parsed field value; discarding it defeats the point of calling it"]
pub unsafe fn extract_module_info(ptr: *const u8) -> ModuleInfoResult<String> {
    if ptr.is_null() {
        return Err(ModuleInfoError::NullPointer);
    }

    // SAFETY: Caller (via `get_module_info!`) passes the address of an
    // `extern "C" static: u8` placed by the linker script inside the
    // `.note.package` payload (read-only for program lifetime, never mutated).
    // Scan forward until NUL; the cap ensures a stripped/missing/corrupted
    // section can't read off the end.
    //
    // Why `MAX_JSON_SIZE + NOTE_ALIGN`: the worst-case symbol sits at byte 0
    // of the first JSON value, so the scan may have to walk the full JSON body
    // (up to `MAX_JSON_SIZE` bytes) plus the `1..=NOTE_ALIGN` NUL padding that
    // `render_note_payloads` always emits after the closing `}` (see
    // metadata.rs). Using `+ NOTE_ALIGN` (not `+ 1`) keeps the bound
    // independent of the exact padding count at no correctness cost.
    const MAX_NOTE_VALUE_LEN: usize = constants::MAX_JSON_SIZE + constants::NOTE_ALIGN;
    let mut length = 0;

    while length < MAX_NOTE_VALUE_LEN {
        let byte = unsafe { *ptr.add(length) };
        if byte == 0 {
            break;
        }
        length += 1;
    }

    if length >= MAX_NOTE_VALUE_LEN {
        // Cap hit: the section is absent, stripped, or corrupted. Surface
        // that instead of a misleading "string too long" error.
        return Err(ModuleInfoError::MalformedJson(format!(
            "No NUL terminator found within {MAX_NOTE_VALUE_LEN} bytes; \
             .note.package section is missing, stripped, or corrupted"
        )));
    }

    let bytes = unsafe { std::slice::from_raw_parts(ptr, length) };
    let str_slice = std::str::from_utf8(bytes)?;

    // Symbol is placed just before the opening `"` of a JSON string literal.
    // Sanitization strips `"` and `\` from values, so a direct slice between
    // the first two quotes is sufficient (no JSON escapes to unescape).
    let open_quote = str_slice
        .find('"')
        .ok_or_else(|| ModuleInfoError::MalformedJson("Missing opening quote".to_string()))?;
    let value_start = open_quote + 1;
    let close_quote_offset = str_slice
        .get(value_start..)
        .and_then(|s| s.find('"'))
        .ok_or_else(|| ModuleInfoError::MalformedJson("Missing closing quote".to_string()))?;
    let value_end = value_start + close_quote_offset;
    let value = str_slice
        .get(value_start..value_end)
        .ok_or_else(|| ModuleInfoError::MalformedJson("Value span out of bounds".to_string()))?;
    Ok(value.to_string())
}

/// Non-Linux stub of [`extract_module_info`]. Always returns
/// `ModuleInfoError::NotAvailable`. The ELF `.note.package` section this
/// reads only exists on Linux, so there's nothing to extract.
///
/// # Safety
/// No safety requirements on this platform: the pointer is never dereferenced
/// (the function returns before touching it). The `unsafe` qualifier is kept
/// only so the signature matches the Linux implementation, letting
/// cross-platform callers use a single call site.
#[cfg(all(feature = "embed-module-info", not(target_os = "linux")))]
#[must_use = "extract_module_info returns the parsed field value; discarding it defeats the point of calling it"]
pub unsafe fn extract_module_info(_ptr: *const u8) -> ModuleInfoResult<String> {
    Err(ModuleInfoError::NotAvailable(
        "Extract module info is only available on Linux platforms with embed-module-info feature."
            .to_string(),
    ))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::{error::Error, fs::File, io::Read, path::Path};

    use tempfile::NamedTempFile;

    use super::*;

    /// Shorthand for tests that propagate with `?`. `Result<(), Box<dyn Error>>`
    /// lets us replace `.expect(...)` with `?` and keeps the test module free
    /// of the workspace-wide `clippy::disallowed_methods` ban on `expect`.
    type TestResult = Result<(), Box<dyn Error>>;

    /// Test-only helper: returns true when `git --version` runs cleanly on
    /// the test host. Tests that depend on a real git checkout (branch/hash
    /// lookup, repo-name parsing) skip gracefully when this returns false so
    /// the suite stays green in stripped-down CI images. Lives inside the
    /// tests module rather than in `utils.rs` so `#[cfg(test)]` doesn't have
    /// to be scattered across production files.
    fn git_is_available() -> bool {
        match std::process::Command::new("git")
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .output()
        {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    #[cfg(feature = "embed-module-info")]
    #[test]
    #[allow(clippy::unnecessary_cast)]
    fn test_extract_module_info() -> TestResult {
        let test_str = "\"test_value\"";
        let c_str = std::ffi::CString::new(test_str)?;
        let ptr = c_str.as_ptr() as *const u8;
        // SAFETY: This is safe because we're creating a valid null-terminated C string
        // using std::ffi::CString which guarantees that the pointer is valid and properly
        // null-terminated for the duration of this function call
        let value = unsafe { extract_module_info(ptr) }?;
        assert_eq!(value, "test_value");
        Ok(())
    }

    #[test]
    fn test_align_len() {
        assert_eq!(utils::align_len(5, NOTE_ALIGN), 8);
        assert_eq!(utils::align_len(8, NOTE_ALIGN), 8);
        assert_eq!(utils::align_len(9, NOTE_ALIGN), 12);
    }

    #[test]
    fn test_get_distro_info() -> TestResult {
        use crate::utils::get_distro_info;
        let distro_info = get_distro_info()?;
        assert!(!distro_info.0.is_empty());
        assert!(!distro_info.1.is_empty());
        Ok(())
    }

    /// The binary note section assembled by `NoteSection::new` must be 4-byte
    /// aligned in total length. The ELF spec requires it, and a misaligned
    /// section silently corrupts subsequent note entries. `NoteSection`
    /// handles this via `align_len` on the owner and desc blocks. This test
    /// exercises desc lengths that stress every residue class mod 4 so a
    /// future refactor that drops the
    /// alignment padding on one of the blocks is caught immediately.
    #[test]
    fn note_section_is_4byte_aligned_for_every_residue() {
        use crate::note_section::NoteSection;
        for desc_len in [0usize, 1, 2, 3, 4, 5, 7, 8, 17, 100, 1023] {
            let desc = "x".repeat(desc_len);
            let note = match NoteSection::new(N_TYPE, OWNER, &desc, "", NOTE_ALIGN) {
                Ok(n) => n,
                Err(e) => panic!("NoteSection::new failed for desc_len={desc_len}: {e}"),
            };
            assert_eq!(
                note.note_section.len() % NOTE_ALIGN,
                0,
                "note section must be 4-byte aligned (desc_len={desc_len}, got {})",
                note.note_section.len()
            );
        }
    }

    #[test]
    fn test_project_metadata() {
        if !git_is_available() {
            println!("Skipping test_project_metadata because git cli is not available");
            return;
        }

        use crate::metadata::project_metadata;
        let result = project_metadata();

        assert!(
            result.is_ok(),
            "Project metadata should be created successfully: {:?}",
            result.err()
        );

        if let Ok(res) = result {
            let metadata = res.0;
            assert!(
                metadata.contains("\"binary\":"),
                "JSON should contain binary field"
            );
            assert!(
                metadata.contains("\"moduleVersion\":"),
                "JSON should contain moduleVersion field"
            );
            assert!(
                metadata.contains("\"version\":"),
                "JSON should contain version field"
            );
            assert!(
                metadata.contains("\"maintainer\":"),
                "JSON should contain maintainer field"
            );
            assert!(
                metadata.contains("\"name\":"),
                "JSON should contain name field"
            );
            assert!(
                metadata.contains("\"type\":"),
                "JSON should contain type field"
            );

            assert!(
                metadata.contains("\"repo\":") || metadata.contains("\"Unknown\""),
                "JSON should contain repo field or fallback"
            );
            assert!(
                metadata.contains("\"branch\":")
                    || metadata.contains("\"main\"")
                    || metadata.contains("\"unknown\""),
                "JSON should contain branch field or fallback"
            );
            assert!(
                metadata.contains("\"hash\":") || metadata.contains("\"unknown\""),
                "JSON should contain hash field or fallback"
            );

            // Other required fields
            assert!(
                metadata.contains("\"copyright\":"),
                "JSON should contain copyright field"
            );
            assert!(metadata.contains("\"os\":"), "JSON should contain os field");
            assert!(
                metadata.contains("\"osVersion\":"),
                "JSON should contain osVersion field"
            );
        }
    }

    /// Exercises the production Cargo.toml-reading path end-to-end against
    /// this crate's own manifest. The assertions are intentionally fork-safe:
    /// an external fork may change `copyright` (and must), but the contract
    /// that Cargo.toml values round-trip through `from_cargo_toml` and
    /// populate the expected fields stays fixed.
    #[test]
    fn test_package_metadata_from_cargo_toml() -> TestResult {
        let md = PackageMetadata::from_cargo_toml()?;

        assert_eq!(md.name, "module-info");
        assert_eq!(md.binary, "module-info");

        // Version is formatted to 3 numeric parts by `format_version_parts`.
        let parts: Vec<&str> = md.version.split('.').collect();
        assert_eq!(
            parts.len(),
            3,
            "version should have three dot-separated parts, got {:?}",
            md.version
        );
        for part in &parts {
            assert!(
                part.chars().all(|c| c.is_ascii_digit()),
                "version part {part:?} must be numeric"
            );
        }

        // `copyright` comes from `[package.metadata.module_info].copyright`
        // in this crate's own Cargo.toml. Forks will legitimately set their
        // own value, so the contract we lock in is "non-empty and not the
        // `Unknown` fallback that triggers when the key is missing",
        // nothing organization-specific.
        assert!(
            !md.copyright.is_empty() && md.copyright != "Unknown",
            "copyright must come from Cargo.toml, not the Unknown fallback; got {:?}",
            md.copyright
        );
        Ok(())
    }

    #[test]
    fn test_get_git_info() -> TestResult {
        if !git_is_available() {
            println!("Skipping test_get_git_info because git is not available");
            return Ok(());
        }

        use crate::utils::get_git_info;
        let git_info = get_git_info()?;

        // Just verify we get back something for the repo name
        // Don't assert exact values since they can change
        // Verify we get back non-empty values
        assert!(!git_info.0.is_empty(), "Branch name should not be empty"); // branch
        assert!(!git_info.1.is_empty(), "Commit hash should not be empty"); // hash
        assert!(
            !git_info.2.is_empty(),
            "Repository name should not be empty"
        ); // repo name

        // If we're in a git repo, it should return a non-"Unknown" value
        // but we accept "Unknown" as valid too (e.g., when testing in a tarball)
        assert!(git_info.2 == "Unknown" || !git_info.2.is_empty());

        println!(
            "Git Info - Branch: {}, Hash: {}, Repo: {}",
            git_info.0, git_info.1, git_info.2
        );
        Ok(())
    }

    #[test]
    fn test_json_key_value_parse() -> TestResult {
        let json_input = r#"{
"binary": "sample_crashing_process",
"moduleVersion": "0.1.0.0",
"version": "0.1.0",
"maintainer": "Maintainer contact/UUID etc",
"name": "sample_crashing_process",
"type": "agent",
"repo": "Module_Info",
"branch": "main",
"hash": "76930c41aa16e31bb1e565b12c4285cde1939af3",
"copyright": "Microsoft",
"os": "Ubuntu",
"osVersion": "20.04"
}
"#;

        let parsed: serde_json::Value = serde_json::from_str(json_input)?;
        assert_eq!(parsed["binary"], "sample_crashing_process");
        assert_eq!(parsed["moduleVersion"], "0.1.0.0");
        assert_eq!(parsed["version"], "0.1.0");
        assert_eq!(parsed["maintainer"], "Maintainer contact/UUID etc");
        assert_eq!(parsed["name"], "sample_crashing_process");
        assert_eq!(parsed["type"], "agent");
        assert_eq!(parsed["repo"], "Module_Info");
        assert_eq!(parsed["branch"], "main");
        assert_eq!(parsed["hash"], "76930c41aa16e31bb1e565b12c4285cde1939af3");
        assert_eq!(parsed["copyright"], "Microsoft");
        assert_eq!(parsed["os"], "Ubuntu");
        assert_eq!(parsed["osVersion"], "20.04");
        Ok(())
    }

    #[test]
    fn test_get_project_path() {
        use crate::utils::get_project_path;
        let project_path = get_project_path();
        assert!(project_path.exists());
    }

    #[test]
    fn test_get_cargo_toml_content() -> TestResult {
        use crate::utils::get_cargo_toml_content;
        let cargo_toml = get_cargo_toml_content()?;
        assert!(cargo_toml.get("package").is_some());
        Ok(())
    }

    #[test]
    fn test_save_section() -> TestResult {
        // Create a temporary file
        let temp_file = NamedTempFile::new()?;
        let file_path = temp_file.path().to_path_buf();

        // Create sample section data
        let desc_json = r#"{"binary":"test","version":"1.0.0"}"#;
        let linker_script_body = "BYTE(0x01); BYTE(0x02);";

        // Create a note section
        use crate::note_section::NoteSection;
        let note = NoteSection::new(N_TYPE, OWNER, desc_json, linker_script_body, NOTE_ALIGN)?;

        // Save the section to the temporary file
        note.save_section(&file_path)?;

        // Read the file back
        let mut file = File::open(&file_path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;

        // Verify the content
        assert!(!buffer.is_empty());
        assert_eq!(buffer.len(), note.note_section.len());
        assert_eq!(buffer, note.note_section);

        // Check that the file contains expected ELF note header values
        // The first 12 bytes should be the ELF note header (n_namesz, n_descsz, n_type)
        assert!(buffer.len() >= 12);

        // Check for the owner string "FDO" followed by null terminator
        let owner_offset = 12; // After the header
        let owner_bytes = OWNER.as_bytes();
        let owner_slice = buffer
            .get(owner_offset..owner_offset + owner_bytes.len())
            .ok_or("owner slice is out of bounds")?;
        assert_eq!(owner_slice, owner_bytes);

        // Ensure the N_TYPE value is present in the header (little endian)
        let n_type_bytes = N_TYPE.to_le_bytes();
        let n_type_slice = buffer.get(8..12).ok_or("n_type slice is out of bounds")?;
        assert_eq!(n_type_slice, &n_type_bytes);
        Ok(())
    }

    /// `PackageMetadata` is public and implements `Default` so callers can
    /// use `..Default::default()` in struct-literal construction. This is
    /// the forward-compatible pattern recommended for build.rs consumers
    /// that supply metadata programmatically.
    #[test]
    fn test_package_metadata_default_construction() {
        let md = PackageMetadata {
            binary: "my_tool".into(),
            name: "my_tool".into(),
            version: "1.2.3".into(),
            module_version: "1.2.3.4".into(),
            maintainer: "team@contoso.com".into(),
            hash: "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef".into(),
            ..Default::default()
        };

        // Fields we set round-trip.
        assert_eq!(md.binary, "my_tool");
        assert_eq!(md.version, "1.2.3");
        assert_eq!(md.module_version, "1.2.3.4");
        // Fields we didn't set come from `Default`: empty strings.
        assert_eq!(md.module_type, "");
        assert_eq!(md.repo, "");
        assert_eq!(md.os, "");
    }

    /// `embed_package_metadata` with a caller-supplied `out_dir` and
    /// `emit_cargo_link_arg = false` must write all three artifacts
    /// (linker script, note bin, JSON) into the specified directory.
    /// This is the static-library flow: the outer build system handles
    /// the final link, so we write artifacts to a known location and
    /// skip the `cargo:rustc-link-arg` directive.
    #[cfg(feature = "embed-module-info")]
    #[test]
    fn test_embed_package_metadata_custom_out_dir_no_link_arg() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let md = PackageMetadata {
            binary: "test_binary".into(),
            name: "test_binary".into(),
            version: "1.2.3".into(),
            module_version: "1.2.3.4".into(),
            maintainer: "team@contoso.com".into(),
            hash: "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef".into(),
            module_type: "agent".into(),
            repo: "test_repo".into(),
            branch: "main".into(),
            copyright: "Test".into(),
            os: "Ubuntu".into(),
            os_version: "22.04".into(),
            ..Default::default()
        };

        let opts = EmbedOptions {
            out_dir: Some(tmp.path().to_path_buf()),
            emit_cargo_link_arg: false,
            ..Default::default()
        };

        let artifacts = embed_package_metadata(&md, &opts)?;

        // All three artifact paths must live under the custom out_dir.
        assert!(artifacts.linker_script_path.starts_with(tmp.path()));
        assert!(artifacts.note_bin_path.starts_with(tmp.path()));
        assert!(artifacts.json_path.starts_with(tmp.path()));

        // And the files must actually exist on disk.
        assert!(artifacts.linker_script_path.exists());
        assert!(artifacts.note_bin_path.exists());
        assert!(artifacts.json_path.exists());

        // And the returned JSON is parseable and contains the supplied values.
        let parsed: serde_json::Value = serde_json::from_str(&artifacts.json)?;
        assert_eq!(parsed["binary"], "test_binary");
        assert_eq!(parsed["version"], "1.2.3");
        assert_eq!(parsed["moduleVersion"], "1.2.3.4");
        Ok(())
    }

    /// `embed_package_metadata` must reject `PackageMetadata` values whose
    /// serialized JSON lacks a required field. Required fields are a safety
    /// guardrail so consumers do not accidentally emit a note section that
    /// `print_module_info` / `get_module_info!` cannot parse. Since
    /// `PackageMetadata` always serializes every field, "missing" in practice
    /// means "empty string". Leave a required field as `Default::default()`
    /// and the validator must reject it.
    #[cfg(feature = "embed-module-info")]
    #[test]
    fn test_embed_package_metadata_rejects_empty_required_field() -> TestResult {
        let tmp = tempfile::tempdir()?;
        // `osVersion` is required; leaving it at `Default::default()` ("")
        // exercises the validator's empty-string rejection path, and its
        // `#[serde(rename = "osVersion")]` mapping is the same one the runtime
        // map consumers see.
        let md = PackageMetadata {
            binary: "b".into(),
            name: "n".into(),
            version: "1.0.0".into(),
            module_version: "1.0.0.0".into(),
            maintainer: "m".into(),
            os: "linux".into(),
            // os_version omitted on purpose; `..Default::default()` gives "".
            ..Default::default()
        };
        let opts = EmbedOptions {
            out_dir: Some(tmp.path().to_path_buf()),
            emit_cargo_link_arg: false,
            ..Default::default()
        };
        let err = embed_package_metadata(&md, &opts)
            .expect_err("embed must reject PackageMetadata with empty required field");
        match err {
            ModuleInfoError::MalformedJson(msg) => {
                assert!(
                    msg.contains("osVersion"),
                    "error must name the empty required field: {msg}"
                );
            }
            other => panic!("expected MalformedJson, got {other:?}"),
        }
        Ok(())
    }

    /// Direct test for the required-field guardrail: feed JSON missing a
    /// required field and confirm it's rejected with a `MalformedJson` error.
    #[test]
    fn test_validate_embedded_json_rejects_missing_required_fields() {
        // Missing "maintainer" (one of the seven required identity-plus-
        // platform keys that stays required even when optional fields like
        // `hash`/`repo`/`branch` are deliberately left empty).
        let bad_json = r#"{"binary":"b","version":"1.0.0","moduleVersion":"1.0.0.0","name":"n"}"#;
        let err =
            validate_embedded_json(bad_json).expect_err("missing required field must be rejected");
        match err {
            ModuleInfoError::MalformedJson(msg) => {
                assert!(
                    msg.contains("maintainer"),
                    "error must name the missing field: {msg}"
                );
            }
            other => panic!("expected MalformedJson, got {other:?}"),
        }
    }

    /// Direct test for the empty-string half of the required-field guardrail.
    /// `PackageMetadata::default()` fields serialize as `""`; we treat that
    /// as "missing" too for the required identity keys, so consumers can't
    /// silently ship a note section with an empty `binary` or `maintainer`.
    /// Non-required fields (hash/repo/branch/type/copyright) are *allowed*
    /// to be empty; that's the documented "disable" knob.
    #[test]
    fn test_validate_embedded_json_rejects_empty_required_fields() {
        // "maintainer" present but empty.
        let bad_json = r#"{"binary":"b","version":"1.0.0","moduleVersion":"1.0.0.0","name":"n","maintainer":""}"#;
        let err =
            validate_embedded_json(bad_json).expect_err("empty required field must be rejected");
        match err {
            ModuleInfoError::MalformedJson(msg) => {
                assert!(
                    msg.contains("maintainer"),
                    "error must name the empty field: {msg}"
                );
            }
            other => panic!("expected MalformedJson, got {other:?}"),
        }
    }

    /// Complement to the rejection tests: a payload that supplies the five
    /// required identity keys but leaves every optional field empty must
    /// pass validation. This pins the "disabled field = empty string"
    /// contract against accidental regressions (e.g., re-adding `hash` to
    /// `REQUIRED_JSON_KEYS`).
    #[test]
    fn test_validate_embedded_json_accepts_empty_optional_fields() {
        let ok_json = r#"{"binary":"b","version":"1.0.0","moduleVersion":"1.0.0.0","name":"n","maintainer":"m","type":"","repo":"","branch":"","hash":"","copyright":"","os":"linux","osVersion":"1"}"#;
        if let Err(e) = validate_embedded_json(ok_json) {
            panic!("optional fields may be empty; only the identity keys are required. got {e:?}");
        }
    }

    /// `EmbedOptions::default()` pins the zero-config behavior:
    /// `out_dir = None` (use `$OUT_DIR`) and `emit_cargo_link_arg = true` so
    /// plain build.rs consumers don't have to set any options.
    #[test]
    fn test_embed_options_default_preserves_bc_behavior() {
        let opts = EmbedOptions::default();
        assert!(opts.out_dir.is_none());
        assert!(opts.emit_cargo_link_arg);
    }

    /// The linker script body must always carry at least one `BYTE(0x00);`
    /// NUL terminator, regardless of the JSON byte-length mod 4. Without it,
    /// `extract_module_info` at runtime would scan past the end of
    /// `.note.package` looking for the sentinel: harmless in practice
    /// (read-only mapped memory) but a latent SIGSEGV risk when the section
    /// sits at a segment boundary. This test constructs a `PackageMetadata`
    /// specifically shaped so the total payload byte-count is a multiple
    /// of 4, which is the tricky case the original `padding_needed = (... % 4)`
    /// formula got wrong (it computed 0 and emitted no padding).
    #[test]
    fn render_note_payloads_always_emits_nul_padding() -> TestResult {
        // Any well-formed PackageMetadata works; we just need the payload.
        // 4-aligned input isn't easy to construct deliberately since the
        // JSON shape mixes fixed keys with variable values, so we assert
        // the stronger "always emits NUL padding" invariant across every
        // permutation of field lengths we can reach with a 2-character probe.
        for suffix_len in 0..=4 {
            let suffix = "x".repeat(suffix_len);
            let md = PackageMetadata {
                binary: format!("b{suffix}"),
                name: format!("n{suffix}"),
                version: "1.0.0".into(),
                module_version: "1.0.0.0".into(),
                maintainer: "m".into(),
                os: "linux".into(),
                os_version: "22.04".into(),
                ..Default::default()
            };
            let (_json, linker_script_body) = crate::metadata::render_note_payloads(&md)?;
            assert!(
                linker_script_body.contains("BYTE(0x00);"),
                "linker script must contain a BYTE(0x00) even when the payload is 4-aligned (suffix_len={suffix_len})"
            );
        }
        Ok(())
    }

    /// `link_arg_directive` is the single branch that decides whether
    /// `cargo:rustc-link-arg=-T<path>` is emitted. Asserting both arms here
    /// locks in the "emit_cargo_link_arg=false means no directive" contract
    /// that static-library flows depend on.
    #[test]
    fn link_arg_directive_gates_on_flag() {
        let p = Path::new("/tmp/linker_script.ld");
        match link_arg_directive(p, true) {
            Some(d) => assert_eq!(d, "cargo:rustc-link-arg=-T/tmp/linker_script.ld"),
            None => panic!("emit_cargo_link_arg=true must produce a directive"),
        }
        assert!(
            link_arg_directive(p, false).is_none(),
            "emit_cargo_link_arg=false must suppress the directive"
        );
    }

    /// Drift guard: every key in `REQUIRED_JSON_KEYS` must appear in
    /// `ModuleInfoField::ALL.to_key()`. If someone adds a required field
    /// without extending the enum (or vice versa), this test fails before
    /// the divergence reaches a consumer.
    #[test]
    fn required_keys_are_subset_of_module_info_fields() {
        let known: std::collections::HashSet<&str> =
            ModuleInfoField::ALL.iter().map(|f| f.to_key()).collect();
        for key in constants::REQUIRED_JSON_KEYS {
            assert!(
                known.contains(key),
                "REQUIRED_JSON_KEYS contains {key:?} which is not in ModuleInfoField::ALL"
            );
        }
    }

    /// `Info` must be constructible from a struct literal (that's the whole
    /// point of the type), and `From<Info> for PackageMetadata` must carry
    /// every field across with the JSON-key-shaped name on the `Info` side and
    /// the snake_case name on the `PackageMetadata` side.
    #[test]
    fn info_struct_literal_and_conversion() {
        let info = Info {
            binary: "b".into(),
            version: "1.2.3".into(),
            moduleVersion: "1.2.3.4".into(),
            maintainer: "m".into(),
            name: "n".into(),
            r#type: "agent".into(),
            repo: "r".into(),
            branch: "br".into(),
            hash: "h".into(),
            copyright: "c".into(),
            os: "o".into(),
            osVersion: "ov".into(),
        };
        let md: PackageMetadata = info.into();
        assert_eq!(md.binary, "b");
        assert_eq!(md.version, "1.2.3");
        assert_eq!(md.module_version, "1.2.3.4");
        assert_eq!(md.maintainer, "m");
        assert_eq!(md.name, "n");
        assert_eq!(md.module_type, "agent");
        assert_eq!(md.repo, "r");
        assert_eq!(md.branch, "br");
        assert_eq!(md.hash, "h");
        assert_eq!(md.copyright, "c");
        assert_eq!(md.os, "o");
        assert_eq!(md.os_version, "ov");
    }

    /// `Info::default()` plus `..Default::default()` struct-literal syntax is
    /// the forward-compatible pattern consumers should use. Unlike
    /// `PackageMetadata`, `Info` is intentionally not `#[non_exhaustive]`, so
    /// both full struct literals and `..Default::default()` must compile and
    /// produce empty strings for unassigned fields.
    #[test]
    fn info_default_fills_missing_fields_with_empty_strings() {
        let info = Info {
            binary: "b".into(),
            moduleVersion: "1.2.3.4".into(),
            ..Default::default()
        };
        assert_eq!(info.binary, "b");
        assert_eq!(info.moduleVersion, "1.2.3.4");
        assert_eq!(info.version, "");
        assert_eq!(info.r#type, "");
        assert_eq!(info.osVersion, "");
    }

    /// `Info` → `PackageMetadata` → `embed_package_metadata` is the path
    /// `new(Info { .. })` takes internally (`new` is just two lines: convert
    /// and dispatch). Exercise it end-to-end with an explicit `out_dir` so
    /// the test doesn't have to mutate `OUT_DIR` on the shared process
    /// environment: `std::env::set_var` is `unsafe fn` on Rust 1.80+ and
    /// racy when tests run in parallel. The actual `new` function is so
    /// thin that the conversion test and this embed test together cover
    /// everything it does.
    #[cfg(feature = "embed-module-info")]
    #[test]
    fn info_embed_round_trip_writes_artifacts() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let md: PackageMetadata = Info {
            binary: "b".into(),
            name: "n".into(),
            version: "1.2.3".into(),
            moduleVersion: "1.2.3.4".into(),
            maintainer: "m".into(),
            r#type: "agent".into(),
            hash: "deadbeef".into(),
            os: "linux".into(),
            osVersion: "22.04".into(),
            ..Default::default()
        }
        .into();

        let opts = EmbedOptions {
            out_dir: Some(tmp.path().to_path_buf()),
            emit_cargo_link_arg: false,
            ..Default::default()
        };
        let artifacts = embed_package_metadata(&md, &opts)?;

        assert!(artifacts.linker_script_path.starts_with(tmp.path()));
        assert!(artifacts.json_path.exists());
        let parsed: serde_json::Value = serde_json::from_str(&artifacts.json)?;
        assert_eq!(parsed["moduleVersion"], "1.2.3.4");
        assert_eq!(parsed["type"], "agent");
        Ok(())
    }

    /// `validate_module_version` accepts the full u16 range on every part.
    #[test]
    fn validate_module_version_accepts_valid_values() -> TestResult {
        for v in ["0.0.0.0", "1.2.3.4", "65535.65535.65535.65535", "10.0.0.1"] {
            validate_module_version(v)?;
        }
        Ok(())
    }

    /// Wrong number of dot-separated parts must fail loudly, not silently
    /// pad or truncate.
    #[test]
    fn validate_module_version_rejects_wrong_part_count() {
        for v in ["", "1", "1.2", "1.2.3", "1.2.3.4.5"] {
            let err = validate_module_version(v).expect_err("wrong part count must be rejected");
            match err {
                ModuleInfoError::MalformedJson(msg) => {
                    assert!(
                        msg.contains("exactly 4"),
                        "error must explain the 4-part rule: {msg}"
                    );
                }
                other => panic!("expected MalformedJson, got {other:?}"),
            }
        }
    }

    /// A u16 overflows at 65536, and consumers parsing the 4-WORD
    /// VS_FIXEDFILEINFO shape would truncate, so reject at embed time.
    #[test]
    fn validate_module_version_rejects_overflow() {
        // 65536 = u16::MAX + 1, on each of the four positions.
        for v in [
            "65536.0.0.0",
            "0.65536.0.0",
            "0.0.65536.0",
            "0.0.0.65536",
            "99999.1.2.3",
        ] {
            let err = validate_module_version(v).expect_err("u16 overflow must be rejected");
            match err {
                ModuleInfoError::MalformedJson(msg) => {
                    assert!(
                        msg.contains("16 bits"),
                        "error must mention the u16 constraint: {msg}"
                    );
                }
                other => panic!("expected MalformedJson, got {other:?}"),
            }
        }
    }

    /// Negative numbers and non-numeric text never fit a u16, and would
    /// silently turn into `0` under lossy casts, so reject them up front.
    #[test]
    fn validate_module_version_rejects_non_numeric() {
        for v in ["-1.0.0.0", "a.b.c.d", "1.2.x.4", "1.2.3.4a", "v1.2.3.4"] {
            validate_module_version(v).expect_err("non-numeric parts must be rejected");
        }
    }

    /// Empty component between dots is rejected explicitly (not just
    /// `parse::<u16>()` fallout) so the error message names the position.
    #[test]
    fn validate_module_version_rejects_empty_part() {
        for v in ["1.2.3.", "1..3.4", "..1.2", "1.2..4"] {
            let err = validate_module_version(v).expect_err("empty part must be rejected");
            if let ModuleInfoError::MalformedJson(msg) = err {
                // Either the part-count check or the empty-part check can
                // fire first depending on the shape; both are acceptable.
                assert!(
                    msg.contains("empty") || msg.contains("exactly 4"),
                    "unexpected error message: {msg}"
                );
            } else {
                panic!("expected MalformedJson");
            }
        }
    }

    /// `validate_embedded_json` must enforce the u16 constraint on
    /// `moduleVersion`, not just the presence check, so the guardrail
    /// applies to every path into `embed_package_metadata`.
    #[test]
    fn validate_embedded_json_rejects_bad_module_version() {
        let bad_json = r#"{"binary":"b","version":"1.0.0","moduleVersion":"1.2.3.99999","name":"n","maintainer":"m","os":"linux","osVersion":"22.04"}"#;
        let err = validate_embedded_json(bad_json)
            .expect_err("out-of-range moduleVersion must be rejected");
        match err {
            ModuleInfoError::MalformedJson(msg) => {
                assert!(
                    msg.contains("moduleVersion"),
                    "error must name the field: {msg}"
                );
            }
            other => panic!("expected MalformedJson, got {other:?}"),
        }
    }

    /// Drift guard: `PackageMetadata::field_value` covers every variant in
    /// `ModuleInfoField::ALL`, and every produced value matches the struct
    /// field serde serializes for the same JSON key. Catches the case where
    /// a new enum variant lands but `field_value` / the struct isn't
    /// extended.
    #[test]
    fn package_metadata_field_value_covers_all_variants() -> TestResult {
        let md = PackageMetadata {
            binary: "bv".into(),
            version: "vv".into(),
            module_version: "mv".into(),
            maintainer: "mn".into(),
            name: "nv".into(),
            module_type: "tv".into(),
            repo: "rv".into(),
            branch: "bn".into(),
            hash: "hv".into(),
            copyright: "cv".into(),
            os: "ov".into(),
            os_version: "ov2".into(),
        };

        let json: serde_json::Value = serde_json::from_str(&serde_json::to_string(&md)?)?;
        for field in ModuleInfoField::ALL {
            let from_method = md.field_value(*field);
            let from_json = json
                .get(field.to_key())
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("JSON missing key for {field:?}"));
            assert_eq!(
                from_method, from_json,
                "field_value and serde output disagree for {field:?}"
            );
        }
        Ok(())
    }
}
