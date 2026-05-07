// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::env;

use serde::{Deserialize, Serialize};

use crate::{
    utils::{bytes_to_linker_directives, get_cargo_toml_content, get_distro_info},
    ModuleInfoError, ModuleInfoField, ModuleInfoResult, NOTE_ALIGN,
};

/// Package metadata for embedding in the ELF `.note.package` section.
///
/// `PackageMetadata` holds the raw (unsanitized) metadata values that will be
/// serialized to JSON and byte-encoded into the linker script by
/// [`embed_package_metadata`](crate::embed_package_metadata). Callers may
/// either populate this struct manually in `build.rs` (e.g. to supply values
/// from an outer build system without touching `Cargo.toml`) or use
/// [`PackageMetadata::from_cargo_toml`] to read the current crate's metadata.
///
/// # Non-exhaustive + Default
///
/// This struct is marked `#[non_exhaustive]` and implements [`Default`] so new
/// fields can be added in future minor releases without breaking downstream
/// code. From outside the crate, `#[non_exhaustive]` forbids struct-literal
/// construction; start from [`Default::default()`] and assign the fields you
/// need:
///
/// ```rust,no_run
/// # use module_info::PackageMetadata;
/// let mut md = PackageMetadata::default();
/// md.maintainer = "team@contoso.com".into();
/// md.module_type = "agent".into();
/// md.version = "1.2.3".into();
/// md.module_version = "1.2.3.4".into();
/// ```
///
/// # Disabling fields
///
/// Seven keys are *required* in the embedded JSON:
/// `binary`, `version`, `moduleVersion`, `name`, `maintainer`, `os`, and
/// `osVersion`. The remaining fields (`type`, `repo`, `branch`, `hash`,
/// `copyright`) are optional. Leave them as the empty string and the
/// corresponding JSON value is emitted as `""`, which downstream tooling
/// can skip. `from_cargo_toml()` populates the `os`/`osVersion` fields
/// from `/etc/os-release`, so most builders get them for free; override
/// only when the detected values don't match the target platform.
///
/// The JSON shape stays stable (every key is always present) because the
/// `.note.package` payload is a fixed-layout byte array built from the
/// linker script; the empty-string-as-disabled convention keeps the layout
/// constant while letting consumers opt out of leaking fields they don't
/// want in the binary.
///
/// ```rust,no_run
/// use module_info::PackageMetadata;
///
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     // Library crate that doesn't want to embed git or repo info:
///     let mut md = PackageMetadata::from_cargo_toml()?;
///     md.repo.clear();
///     md.branch.clear();
///     md.hash.clear();
///     // `md` still carries binary/version/moduleVersion/name/maintainer
///     // plus os/osVersion (auto-populated from /etc/os-release).
///     Ok(())
/// }
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PackageMetadata {
    /// Binary name (executable or library)
    pub binary: String,

    /// Full module version (may include build number)
    #[serde(rename = "moduleVersion")]
    pub module_version: String,

    /// Crate version from Cargo.toml
    pub version: String,

    /// Maintainer contact information
    pub maintainer: String,

    /// Package name
    pub name: String,

    /// Module type (agent, library, executable, etc.)
    #[serde(rename = "type")] // Ensure JSON uses "type" instead of "module_type"
    pub module_type: String,

    /// Git repository name
    pub repo: String,

    /// Git branch name
    pub branch: String,

    /// Git commit hash
    pub hash: String,

    /// Copyright information
    pub copyright: String,

    /// Operating system name
    pub os: String,

    /// Operating system version
    #[serde(rename = "osVersion")]
    pub os_version: String,
}

impl PackageMetadata {
    /// Build a [`PackageMetadata`] by reading the current crate's `Cargo.toml`,
    /// environment-variable overrides, git working copy, and OS release info.
    ///
    /// This is the zero-configuration entry point: the build script for a
    /// normal Cargo crate can just call
    /// [`generate_project_metadata_and_linker_script`](crate::generate_project_metadata_and_linker_script),
    /// which uses this method under the hood. Call `from_cargo_toml` directly
    /// only when you need to inspect or mutate the collected metadata before
    /// passing it to [`embed_package_metadata`](crate::embed_package_metadata).
    ///
    /// The returned values are *unsanitized*. `embed_package_metadata` runs
    /// the sanitize step internally so the linker-script bytes and the JSON
    /// string agree byte-for-byte (the invariant that keeps the `.note.package`
    /// section 4-byte aligned).
    ///
    /// # Errors
    /// Returns a [`ModuleInfoError`] if `Cargo.toml` is unreadable or malformed,
    /// if git invocation fails, or if the OS release info cannot be read.
    pub fn from_cargo_toml() -> ModuleInfoResult<Self> {
        collect_package_metadata()
    }

    /// Return the string value associated with a given [`ModuleInfoField`].
    ///
    /// This is the single source of truth mapping `ModuleInfoField` variants
    /// to `PackageMetadata` fields. Both the linker-script emitter and the
    /// build-time JSON dump iterate [`ModuleInfoField::ALL`] and call this.
    #[must_use]
    pub fn field_value(&self, field: ModuleInfoField) -> &str {
        match field {
            ModuleInfoField::Binary => &self.binary,
            ModuleInfoField::Version => &self.version,
            ModuleInfoField::ModuleVersion => &self.module_version,
            ModuleInfoField::Maintainer => &self.maintainer,
            ModuleInfoField::Name => &self.name,
            ModuleInfoField::Type => &self.module_type,
            ModuleInfoField::Repo => &self.repo,
            ModuleInfoField::Branch => &self.branch,
            ModuleInfoField::Hash => &self.hash,
            ModuleInfoField::Copyright => &self.copyright,
            ModuleInfoField::Os => &self.os,
            ModuleInfoField::OsVersion => &self.os_version,
        }
    }
}

/// Look up `package.metadata.module_info.<key>` as a string slice.
fn module_info_str<'a>(package: &'a toml::Value, key: &str) -> Option<&'a str> {
    package
        .get("metadata")
        .and_then(|m| m.get("module_info"))
        .and_then(|mi| mi.get(key))
        .and_then(|v| v.as_str())
}

/// Normalize a dotted version string to exactly `parts` numeric components,
/// padding missing trailing components with `0` and truncating extras.
///
/// SemVer-style pre-release / build-metadata suffixes (everything from the
/// first `-` or `+`) are stripped before splitting so that Azure Pipelines
/// build numbers like `"5.2.100.0-PullRequest-123456"` normalize cleanly
/// to `"5.2.100.0"` (or `"5.2.100"` when `parts == 3`) instead of leaving a
/// non-numeric tail that would fail the u16 check in
/// `validate_module_version`.
fn format_version_parts(version_str: &str, parts: usize) -> String {
    // Strip pre-release / build-metadata suffix before splitting. Find the
    // first `-` or `+` (whichever comes first) and cut there.
    let cut = match (version_str.find('-'), version_str.find('+')) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };
    let core = match cut {
        Some(end) => version_str.get(..end).unwrap_or(version_str),
        None => version_str,
    };
    if core.len() != version_str.len() {
        warn!(
            "version string {:?} carries pre-release/build-metadata suffix; using numeric core {:?}",
            version_str, core
        );
    }
    // Treat an empty core as "no dot-separated numeric input" rather than
    // "one empty component": `"".split('.')` returns `[""]`, which would
    // otherwise make the first emitted part the empty string and produce
    // a malformed result like ".0.0" (for parts=3) or ".0.0.0" (for parts=4).
    // The downstream u16 validator in `validate_module_version` would then
    // reject with a confusing "part 0 is empty" error instead of the
    // expected "0.0.0" / "0.0.0.0" fallback.
    let fields: Vec<&str> = if core.is_empty() {
        Vec::new()
    } else {
        core.split('.').collect()
    };
    if fields.len() > parts {
        // Truncation is warn-not-error for backwards compat with pipelines
        // whose build numbers incidentally carry extra dots (e.g. a
        // `BUILD_BUILDNUMBER` of `"1.2.3.4.5"` trimmed to `"1.2.3.4"`).
        warn!(
            "version string {:?} has {} dot-separated parts; truncating to {} (dropped: {:?})",
            core,
            fields.len(),
            parts,
            fields.get(parts..).map(|s| s.join(".")).unwrap_or_default()
        );
    }
    // Warn early when any part overflows u16: the hard check in
    // `validate_module_version` will still reject the value later, but the
    // error then surfaces several call-frames deep in `embed_package_metadata`
    // as a generic "moduleVersion part N must fit in 16 bits". Warning here
    // points at the actual offending env var / Cargo.toml value in the CI log.
    for (i, f) in fields.iter().take(parts).enumerate() {
        if !f.is_empty() && f.parse::<u16>().is_err() {
            warn!(
                "version part {} ({:?}) in {:?} does not fit u16; downstream validate_module_version will reject this build",
                i, f, core
            );
        }
    }
    (0..parts)
        .map(|i| fields.get(i).copied().unwrap_or("0"))
        .collect::<Vec<_>>()
        .join(".")
}

/// Read `$env_var_name` (if set) and return its trimmed value, or `fallback`
/// when the env var is unset, unreadable, or whitespace-only.
///
/// Trimming matters because CI-supplied values (e.g. `BUILD_BUILDNUMBER`)
/// occasionally arrive with stray leading/trailing whitespace, which would
/// otherwise propagate into the first `.`-separated field of a version
/// string and fail the u16 range check in `validate_module_version`.
fn env_or_default(env_var_name: Option<&str>, fallback: &str) -> String {
    let Some(name) = env_var_name else {
        return fallback.to_string();
    };
    let value = match env::var(name) {
        Ok(v) => v,
        Err(env::VarError::NotPresent) => String::new(),
        Err(env::VarError::NotUnicode(_)) => {
            // Non-UTF8 env values silently drop to the fallback rather than
            // poisoning the embedded JSON with replacement characters. A
            // cargo:warning keeps the root cause visible at build time.
            println!(
                "cargo:warning=module_info: env var {name} contains non-UTF8 bytes; using fallback"
            );
            String::new()
        }
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Read Cargo.toml, env vars, git, and OS release info and populate a raw
/// (unsanitized) `PackageMetadata`.
fn collect_package_metadata() -> ModuleInfoResult<PackageMetadata> {
    let cargo_toml = get_cargo_toml_content()?;
    let package = cargo_toml
        .get("package")
        .ok_or_else(|| ModuleInfoError::MalformedJson("No package section found".to_string()))?;

    let binary_name = env::var("CARGO_PKG_NAME").unwrap_or_default();
    let default_version = env::var("CARGO_PKG_VERSION").unwrap_or_default();

    let version_env_var_name = module_info_str(package, "version_env_var_name").map(str::to_string);
    let module_version_env_var_name =
        module_info_str(package, "module_version_env_var_name").map(str::to_string);

    // Caller-named env vars: emit rerun-if-env-changed here since
    // `embed_package_metadata`'s fixed rerun set can't know them.
    if let Some(name) = version_env_var_name.as_deref() {
        println!("cargo:rerun-if-env-changed={name}");
    }
    if let Some(name) = module_version_env_var_name.as_deref() {
        println!("cargo:rerun-if-env-changed={name}");
    }

    let raw_version = env_or_default(version_env_var_name.as_deref(), &default_version);
    let version = format_version_parts(&raw_version, 3);
    let raw_module_version = env_or_default(module_version_env_var_name.as_deref(), &raw_version);
    let module_version = format_version_parts(&raw_module_version, 4);

    let (branch, hash, repo) = crate::utils::get_git_info()?;

    let maintainer = module_info_str(package, "maintainer")
        .unwrap_or("Unknown")
        .to_string();
    let module_type = module_info_str(package, "type")
        .unwrap_or("Unknown")
        .to_string();
    let copyright = module_info_str(package, "copyright")
        .unwrap_or("Unknown")
        .to_string();

    let (os, os_version) = get_distro_info()?;

    // Return unsanitized values; `render_note_payloads` sanitizes at emit time
    // so both manual and `from_cargo_toml` construction paths agree byte-for-byte.
    Ok(PackageMetadata {
        binary: binary_name.clone(),
        module_version,
        version,
        maintainer,
        name: binary_name,
        module_type,
        repo,
        branch,
        hash,
        copyright,
        os,
        os_version,
    })
}

/// Render a [`PackageMetadata`] into the two byte-identical payloads embedded
/// in the ELF note section: the compact JSON string (`.0`) and the
/// linker-script body that reproduces the same bytes (`.1`).
///
/// Sanitization happens here. The returned JSON string and the byte-encoded
/// linker-script body are guaranteed to agree byte-for-byte, which is what
/// keeps the `.note.package` section 4-byte aligned.
pub(crate) fn render_note_payloads(md: &PackageMetadata) -> ModuleInfoResult<(String, String)> {
    // Sanitize before serialization so JSON bytes and linker bytes agree.
    // Otherwise characters that expand/strip (`©` → `(c)`, non-ASCII) would
    // drift padding and break 4-byte alignment of the note section.
    let metadata = PackageMetadata {
        binary: sanitize_for_linker_script(&md.binary),
        module_version: sanitize_for_linker_script(&md.module_version),
        version: sanitize_for_linker_script(&md.version),
        maintainer: sanitize_for_linker_script(&md.maintainer),
        name: sanitize_for_linker_script(&md.name),
        module_type: sanitize_for_linker_script(&md.module_type),
        repo: sanitize_for_linker_script(&md.repo),
        branch: sanitize_for_linker_script(&md.branch),
        hash: sanitize_for_linker_script(&md.hash),
        copyright: sanitize_for_linker_script(&md.copyright),
        os: sanitize_for_linker_script(&md.os),
        os_version: sanitize_for_linker_script(&md.os_version),
    };

    // Emit JSON and linker directives in lock-step so byte counts agree.
    // Manually emit newlines (not `serde_json::to_string`, which emits one line)
    // so `strings`/`readelf -n` show one key:value pair per line.
    let mut linker_script_body = String::new();
    let mut compact_json = String::new();

    // Iterate `ModuleInfoField::ALL` so the emitter stays in lock-step with the
    // enum (exhaustive iteration surfaces missing/extra keys at compile time).
    let entries: Vec<(&str, &str, &str)> = ModuleInfoField::ALL
        .iter()
        .map(|f| (f.to_key(), f.to_symbol_name(), metadata.field_value(*f)))
        .collect();

    // Derive padding from this running count, not `compact_json.len()`.
    // Sanitized bytes *should* match `compact_json.len()` (the hard check
    // below enforces that), but using the emitted byte count is the invariant
    // that actually keeps `.note.package` 4-byte aligned. If sanitization
    // ever drifts string length, the emitted count is still authoritative.
    let mut note_payload_bytes: usize = 0;

    linker_script_body.push('\n');
    linker_script_body.push_str(&bytes_to_linker_script_format("{\n")); // '{', '\n'
    compact_json.push_str("{\n");
    note_payload_bytes += 2;
    for (i, (key, symbol_name, value)) in entries.iter().enumerate() {
        let key_json = format!("\"{key}\":");
        let bytes_key_str = bytes_to_linker_script_format(&key_json);
        linker_script_body.push_str(&format!("\n\n    /* Key: {key} */"));
        linker_script_body.push_str(&format!("\n{bytes_key_str}"));
        compact_json.push_str(&key_json);
        note_payload_bytes += key_json.len();

        // Symbol marks the value's start address; runtime extraction reads from
        // here without parsing the JSON.
        linker_script_body.push_str(&format!("\n    {symbol_name} = .;"));

        // No `/* Value: ... */` comment: sanitized values could contain `*/`
        // and interpolating user bytes into a C-style comment is fragile.
        let value_json = format!("\"{value}\"");
        let bytes_value_str = bytes_to_linker_script_format(&value_json);
        linker_script_body.push_str(&format!("\n{bytes_value_str}"));
        compact_json.push_str(&value_json);
        note_payload_bytes += value_json.len();

        if i < entries.len() - 1 {
            linker_script_body.push('\n');
            linker_script_body.push_str(&bytes_to_linker_script_format(",\n"));
            compact_json.push_str(",\n");
            note_payload_bytes += 2;
        }
    }

    linker_script_body.push('\n');
    linker_script_body.push_str(&bytes_to_linker_script_format("\n}")); // '\n', '}'
    compact_json.push_str("\n}");
    note_payload_bytes += 2;
    debug!(" Compact JSON Len: {}", compact_json.len());

    // Always emit 1–4 NUL bytes of padding (never 0). The lower bound is
    // load-bearing: `extract_module_info` scans forward until `\0`, capped at
    // `MAX_NOTE_VALUE_LEN`. Without a terminating NUL the scan runs to the cap
    // and reads bytes past the section: harmless mid-segment, SIGSEGV at a
    // segment tail. When `note_payload_bytes % NOTE_ALIGN == 0`, the formula
    // below emits a full `NOTE_ALIGN` (4-byte) NUL pad, *not* 0; the section
    // stays 4-aligned either way (+0 mod 4 vs. +4 mod 4), and the scan still
    // terminates on the first NUL.
    let padding_needed = NOTE_ALIGN - (note_payload_bytes % NOTE_ALIGN);
    // Hard check (not debug_assert): mismatch here means the linker script and
    // JSON disagree byte-for-byte, which corrupts alignment and breaks runtime
    // extraction. Return an error (not panic) so build.rs sees a clean exit.
    if note_payload_bytes != compact_json.len() {
        return Err(crate::ModuleInfoError::Other(
            format!(
                "linker script payload size ({note_payload_bytes}) disagrees with compact_json ({}); \
                 sanitizer and emitter drifted out of sync",
                compact_json.len()
            )
            .into(),
        ));
    }

    // Always runs; `padding_needed` is in [1, 4] by construction above.
    linker_script_body.push_str("\n    /* Padding (always >=1 NUL so runtime scan terminates) */");
    for _ in 0..padding_needed {
        linker_script_body.push('\n');
        linker_script_body.push_str(&bytes_to_linker_script_format("\0"));
    }

    debug!("Linker script body:\n{}", linker_script_body);
    debug!("Compact JSON:\n{}", compact_json);
    debug!("Linker script body size: {}", linker_script_body.len());
    debug!("Compact JSON size: {}", compact_json.len());
    debug!("Padding needed: {}", padding_needed);
    debug!(
        "Linker script body size after padding: {}",
        linker_script_body.len()
    );
    debug!("Compact JSON size after padding: {}", compact_json.len());

    Ok((compact_json, linker_script_body))
}

/// Thin wrapper that chains [`PackageMetadata::from_cargo_toml`] and
/// [`render_note_payloads`], retained so the `test_project_metadata`
/// regression test can exercise both stages through one call. Production code
/// reaches for [`crate::embed_package_metadata`] instead.
#[cfg(test)]
pub(crate) fn project_metadata() -> ModuleInfoResult<(String, String)> {
    let md = PackageMetadata::from_cargo_toml()?;
    render_note_payloads(&md)
}

/// Sanitize a string so that it can be embedded verbatim in both the emitted
/// linker script (as raw bytes) and the compact JSON metadata (via serde_json)
/// without the two representations disagreeing in length.
///
/// The contract is: after sanitization, `serde_json::to_string` of the value
/// produces the same bytes the linker will emit. That invariant keeps the
/// `.note.package` section 4-byte aligned (padding is computed from the
/// emitted-byte count) and keeps the JSON parseable at runtime.
///
/// To achieve that we strip every character that `serde_json` would escape:
/// - `"` (would become `\"` in JSON: 2 bytes vs 1 raw byte)
/// - `\` (would become `\\`)
/// - any control character, including `\n`, `\r`, `\t` (would become `\n`/`\t`/`\uNNNN`)
/// - any non-ASCII character (would be UTF-8 multi-byte; we keep the section ASCII-only)
///
/// We also map a few common trademark/copyright glyphs to ASCII equivalents
/// before stripping, because those legitimately show up in `copyright` fields.
/// These replacements *expand* the byte count (`©` = 2 UTF-8 bytes → `(c)` =
/// 3 ASCII bytes), so a pathological copyright string full of glyphs can push
/// the serialized JSON over [`crate::constants::MAX_JSON_SIZE`] (1 KiB) and
/// fail validation in [`crate::embed_package_metadata`]. That's the intended
/// outcome (the size cap is the forcing function), but if a build fails
/// with `MetadataTooLarge`, glyph expansion is the likely cause.
///
/// The glyph map is intentionally minimal (©, ®, ™). Other non-ASCII glyphs
/// that might appear in author or copyright fields (em dash (`—`), curly
/// quotes (`“ ”`), section (`§`), accented Latin letters (`André`), or any
/// CJK text) are dropped rather than transliterated. Expand the map if a
/// legitimate field is losing characters, and update the
/// `sanitize_strips_...` tests to match. Don't add per-language
/// transliteration (`é` → `e`): that loses meaning silently. Prefer an ASCII
/// spelling at the source.
///
/// Additionally, `*` and `/` are preserved (they appear in paths and versions),
/// but the caller must not interpolate the sanitized string into a C-style
/// `/* ... */` comment without escaping `*/` first.
///
/// # Example
/// `"Contoso©"` → `"Contoso(c)"`; `"a\"b\nc"` → `"abc"`.
pub fn sanitize_for_linker_script(input: &str) -> String {
    input
        .replace('©', "(c)")
        .replace('®', "(r)")
        .replace('™', "(tm)")
        .chars()
        .filter(|&c| {
            // Must be plain ASCII so the emitted bytes are one-to-one with chars.
            if !c.is_ascii() {
                return false;
            }
            // Drop anything serde_json would escape, and anything that would
            // otherwise break the emitted JSON string literal at runtime.
            if c.is_control() {
                return false;
            }
            if c == '"' || c == '\\' {
                return false;
            }
            // Keep printable ASCII: alphanumeric, space, and the standard
            // punctuation set (minus the quote / backslash we excluded above).
            c.is_alphanumeric() || c == ' ' || c.is_ascii_punctuation()
        })
        .collect()
}

/// `&str` convenience wrapper over [`bytes_to_linker_directives`].
fn bytes_to_linker_script_format(s: &str) -> String {
    bytes_to_linker_directives(s.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// After sanitizing, the bytes `serde_json::to_string` emits for the value
    /// must equal the raw bytes the linker script writes (the value wrapped in
    /// `"..."`). This is the invariant that keeps the `.note.package` section
    /// 4-byte aligned: the linker-script padding is computed from the raw
    /// byte count, so any divergence would corrupt the ELF note alignment.
    fn assert_sanitize_json_agreement(raw: &str) {
        let sanitized = sanitize_for_linker_script(raw);
        let linker_bytes = format!("\"{sanitized}\"");
        // `serde_json::to_string` on a `String` cannot fail; match explicitly
        // rather than using an unreachable `.unwrap_or_default()`.
        let json_bytes = match serde_json::to_string(&sanitized) {
            Ok(s) => s,
            Err(e) => panic!("serde_json::to_string on a plain String failed: {e}"),
        };
        assert_eq!(
            linker_bytes, json_bytes,
            "sanitized input {raw:?} produced diverging linker vs. JSON bytes"
        );
        assert_eq!(
            linker_bytes.len(),
            json_bytes.len(),
            "sanitized input {raw:?} produced diverging byte lengths"
        );
    }

    #[test]
    fn sanitize_strips_quote_and_backslash() {
        // `"` and `\` would both be escaped by serde_json, doubling their byte
        // count; they must be stripped so the sanitized value serializes verbatim.
        let s = sanitize_for_linker_script("a\"b\\c");
        assert_eq!(s, "abc");
        assert_sanitize_json_agreement("a\"b\\c");
    }

    #[test]
    fn sanitize_strips_control_chars() {
        // Newlines, carriage returns, tabs, and the NUL byte would all be escaped
        // by serde_json (\n, \r, \t, \u0000); strip them to keep byte counts aligned.
        let s = sanitize_for_linker_script("line1\nline2\r\nx\ty\0z");
        assert_eq!(s, "line1line2xyz");
        assert_sanitize_json_agreement("line1\nline2\r\nx\ty\0z");
    }

    #[test]
    fn sanitize_maps_common_glyphs_to_ascii() {
        // Copyright/trademark glyphs are the realistic case in a `copyright`
        // field; they must round-trip to ASCII before the non-ASCII filter runs.
        let s = sanitize_for_linker_script("Contoso© Fabrikam® / Widgets™");
        assert_eq!(s, "Contoso(c) Fabrikam(r) / Widgets(tm)");
        assert_sanitize_json_agreement("Contoso© Fabrikam® / Widgets™");
    }

    #[test]
    fn sanitize_strips_generic_non_ascii() {
        // Generic non-ASCII (e.g. accented names in an author field) would be
        // emitted as multi-byte UTF-8 by the linker but escaped as \uNNNN by
        // serde_json unless it stays in the BMP; either way the byte counts
        // diverge, so non-ASCII must be dropped.
        let s = sanitize_for_linker_script("André naïve 日本");
        assert_eq!(s, "Andr nave ");
        assert_sanitize_json_agreement("André naïve 日本");
    }

    #[test]
    fn sanitize_preserves_star_slash_for_paths_and_versions() {
        // `*` and `/` are intentionally kept; they appear in paths and in
        // version strings. (The linker-script emitter deliberately does NOT
        // interpolate sanitized values into `/* ... */` comments, so `*/`
        // in a value cannot close a comment.)
        let s = sanitize_for_linker_script("path/to/*.rs v1.2.3+build");
        assert_eq!(s, "path/to/*.rs v1.2.3+build");
        assert_sanitize_json_agreement("path/to/*.rs v1.2.3+build");
    }

    #[test]
    fn sanitize_keeps_star_slash_sequence_literally() {
        // Regression guard: even if a value contains `*/`, sanitize keeps both
        // characters (they're not safety-critical on their own). The safety
        // comes from the linker-script emitter never interpolating the value
        // into a C-style comment. The JSON and linker byte counts still match.
        let raw = "hello*/world";
        let s = sanitize_for_linker_script(raw);
        assert_eq!(s, "hello*/world");
        assert_sanitize_json_agreement(raw);
    }

    #[test]
    fn sanitize_handles_empty_string() {
        let s = sanitize_for_linker_script("");
        assert_eq!(s, "");
        assert_sanitize_json_agreement("");
    }

    #[test]
    fn sanitize_handles_only_stripped_chars() {
        // All-bad input should collapse to empty, and empty must still agree
        // across JSON and linker output (both emit `""`, 2 bytes).
        let s = sanitize_for_linker_script("\"\\\n\t日");
        assert_eq!(s, "");
        assert_sanitize_json_agreement("\"\\\n\t日");
    }

    /// Azure Pipelines' `BUILD_BUILDNUMBER` can arrive shaped like
    /// `"5.2.100.0-PullRequest-123456"`. The SemVer-style `-<suffix>` must
    /// be stripped before splitting on `.`, otherwise the 4-part
    /// `moduleVersion` fails the u16 check on the last component.
    #[test]
    fn format_version_parts_strips_semver_suffix() {
        assert_eq!(
            format_version_parts("5.2.100.0-PullRequest-123456", 4),
            "5.2.100.0"
        );
        assert_eq!(
            format_version_parts("5.2.100.0-PullRequest-123456", 3),
            "5.2.100"
        );
        // SemVer pre-release label (`-beta.N`) is also stripped.
        assert_eq!(format_version_parts("2.10.0-beta.3", 4), "2.10.0.0");
        // `+build` (SemVer build-metadata) is also stripped.
        assert_eq!(format_version_parts("3.1.4+ci.42", 3), "3.1.4");
        // Plain numeric input is unchanged.
        assert_eq!(format_version_parts("1.2.3.4", 4), "1.2.3.4");
        // Padding behavior is preserved for short inputs.
        assert_eq!(format_version_parts("1.2", 4), "1.2.0.0");
        // Empty input must yield the all-zero fallback (not ".0.0" /
        // ".0.0.0" from `"".split('.')` returning `[""]`). A malformed
        // leading-dot result would fail downstream u16 validation with
        // a confusing "part 0 is empty" error.
        assert_eq!(format_version_parts("", 3), "0.0.0");
        assert_eq!(format_version_parts("", 4), "0.0.0.0");
    }

    #[test]
    fn sanitize_is_idempotent() {
        // Applying sanitize repeatedly must produce the same result as applying
        // it once. The invariant described in the doc comment (JSON and
        // linker-script bytes agree) could drift through code paths that
        // re-sanitize defensively, so we iterate four passes rather than the
        // original two; enough to catch a rewrite where a glyph expansion
        // introduces another glyph-trigger pattern.
        //
        // The input deliberately includes:
        // - Raw glyphs that get expanded (`©`→`(c)`, `®`→`(r)`, `™`→`(tm)`)
        // - Text that matches the *output* of those expansions
        //   (`"Copyright (c) Contoso (2024)"`), which is the scenario where
        //   a regex-based rewrite could go wrong by re-triggering expansion
        //   on the `(c)` literal. The current implementation uses literal
        //   `.replace()` so it's immune; the test pins that contract.
        let inputs = [
            "Contoso© Fabrikam® Widgets™ / path*/here",
            "Copyright (c) Contoso (2024), (r) (tm)",
            "(c)(r)(tm) only",
            "",
        ];
        for raw in inputs {
            let once = sanitize_for_linker_script(raw);
            let mut current = once.clone();
            for pass in 2..=4 {
                let next = sanitize_for_linker_script(&current);
                assert_eq!(
                    next, once,
                    "sanitize pass {pass} for input {raw:?} diverged from pass 1 output {once:?}; got {next:?}"
                );
                current = next;
            }
        }
    }
}
