// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::fmt;

use cfg_if::cfg_if;

/// Errors returned from `module_info` APIs.
///
/// `#[non_exhaustive]`: new variants may land in minor releases. Any `match`
/// on this enum from outside the crate needs a wildcard arm or it will
/// fail to compile when a variant is added.
///
/// # Example
///
/// ```
/// use module_info::{ModuleInfoError, ModuleInfoResult};
///
/// // A function that might return a ModuleInfoError
/// fn get_module_name() -> ModuleInfoResult<String> {
///     Err(ModuleInfoError::NotAvailable("example".to_string()))
/// }
///
/// match get_module_name() {
///     Ok(name) => println!("Module name: {name}"),
///     Err(ModuleInfoError::NotAvailable(msg)) => eprintln!("not available: {msg}"),
///     Err(ModuleInfoError::NullPointer) => eprintln!("null pointer"),
///     Err(ModuleInfoError::MalformedJson(msg)) => eprintln!("malformed JSON: {msg}"),
///     Err(e) => eprintln!("other error: {e}"),
/// }
/// ```
#[derive(Debug)]
#[non_exhaustive]
pub enum ModuleInfoError {
    /// Module info is unavailable: either the `embed-module-info` feature is
    /// off or the target is not Linux. The contained string carries context.
    NotAvailable(String),

    /// A null pointer was passed to `extract_module_info`. Typically means
    /// the linker script did not run or the `.note.package` section was
    /// stripped from the binary.
    NullPointer,

    /// The embedded bytes were not valid UTF-8.
    Utf8Error(std::str::Utf8Error),

    /// The embedded JSON could not be parsed, a required field is missing
    /// or empty, or `moduleVersion` is not four `u16`-sized parts. The
    /// contained string identifies the specific failure.
    MalformedJson(String),

    /// The serialized metadata JSON exceeded `MAX_JSON_SIZE` (1 KiB) at
    /// build time. The contained string reports the actual vs. allowed size.
    MetadataTooLarge(String),

    /// I/O failure while reading `Cargo.toml` or writing the generated
    /// linker script and JSON dump from `build.rs`.
    IoError(std::io::Error),

    /// Catch-all for errors that do not fit the variants above. Holds the
    /// originating error for `source()` chaining.
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl fmt::Display for ModuleInfoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModuleInfoError::NotAvailable(msg) => write!(f, "Module info not available: {msg}"),
            ModuleInfoError::NullPointer => write!(f, "Pointer is null"),
            ModuleInfoError::Utf8Error(err) => write!(f, "UTF-8 conversion error: {err}"),
            ModuleInfoError::MalformedJson(msg) => write!(f, "Malformed JSON string: {msg}"),
            ModuleInfoError::MetadataTooLarge(msg) => {
                write!(f, "Metadata size exceeds limit: {msg}")
            }
            ModuleInfoError::IoError(err) => write!(f, "IO error: {err}"),
            ModuleInfoError::Other(err) => write!(f, "Other error: {err}"),
        }
    }
}

impl std::error::Error for ModuleInfoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ModuleInfoError::Utf8Error(err) => Some(err),
            ModuleInfoError::IoError(err) => Some(err),
            ModuleInfoError::Other(err) => Some(err.as_ref()),
            _ => None,
        }
    }
}

impl From<std::str::Utf8Error> for ModuleInfoError {
    fn from(err: std::str::Utf8Error) -> Self {
        ModuleInfoError::Utf8Error(err)
    }
}

impl From<std::io::Error> for ModuleInfoError {
    fn from(err: std::io::Error) -> Self {
        ModuleInfoError::IoError(err)
    }
}

impl From<std::env::VarError> for ModuleInfoError {
    fn from(err: std::env::VarError) -> Self {
        ModuleInfoError::Other(Box::new(err))
    }
}

// Conditionally compile the toml and serde_json error conversions only for Linux
cfg_if! {
    if #[cfg(target_os = "linux")] {
        impl From<toml::de::Error> for ModuleInfoError {
            fn from(err: toml::de::Error) -> Self {
                ModuleInfoError::Other(Box::new(err))
            }
        }

        impl From<serde_json::Error> for ModuleInfoError {
            fn from(err: serde_json::Error) -> Self {
                ModuleInfoError::Other(Box::new(err))
            }
        }
    }
}

/// A type alias for Results that use ModuleInfoError
pub type ModuleInfoResult<T> = Result<T, ModuleInfoError>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

    /// Sweep every variant once: build it, run `Display::fmt`, and call
    /// `Error::source`. Together this exercises every match arm in
    /// `Display::fmt` and every arm in `source()` that returns a
    /// concrete error vs. `None`. Without it `error.rs` shows 0%
    /// coverage in `cargo llvm-cov` because variants are *constructed*
    /// elsewhere via `?`/`From` impls but never *displayed*.
    #[test]
    fn display_and_source_cover_every_variant() {
        // Use a runtime-built byte slice so `clippy::invalid_utf8_in_unchecked`
        // (and the related `invalid-from-utf8` lint that fires on a literal
        // `&[0xff]`) doesn't flag the test as obviously-erroring at compile
        // time. The bytes are still always invalid UTF-8 at runtime.
        let invalid_utf8: Vec<u8> = vec![0xff, 0xfe];
        let utf8_err = match std::str::from_utf8(&invalid_utf8) {
            Ok(_) => unreachable!("invalid_utf8 is never valid UTF-8"),
            Err(e) => e,
        };

        // For each variant, assert (a) Display produces a non-empty
        // string with the expected prefix, and (b) `source()` returns
        // Some/None per the doc contract.
        let cases: Vec<(ModuleInfoError, &str, bool)> = vec![
            (
                ModuleInfoError::NotAvailable("ctx".into()),
                "Module info not available",
                false,
            ),
            (ModuleInfoError::NullPointer, "Pointer is null", false),
            (
                ModuleInfoError::MalformedJson("bad".into()),
                "Malformed JSON string",
                false,
            ),
            (
                ModuleInfoError::MetadataTooLarge("size".into()),
                "Metadata size exceeds limit",
                false,
            ),
            // Variants whose source() returns the inner error:
            (
                ModuleInfoError::Utf8Error(utf8_err),
                "UTF-8 conversion error",
                true,
            ),
            (
                ModuleInfoError::IoError(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "missing",
                )),
                "IO error",
                true,
            ),
            (
                ModuleInfoError::Other(std::io::Error::other("boxed").into()),
                "Other error",
                true,
            ),
        ];
        for (err, prefix, has_source) in cases {
            let rendered = format!("{err}");
            assert!(
                rendered.starts_with(prefix),
                "Display for {err:?} should start with {prefix:?}, got {rendered:?}"
            );
            assert_eq!(
                err.source().is_some(),
                has_source,
                "source() arm wrong for {err:?}"
            );
        }
    }

    /// `From<std::str::Utf8Error>` and `From<std::io::Error>` are the
    /// auto-conversions `?` exercises throughout the crate. Hit them
    /// directly here so coverage doesn't silently drop if a future
    /// refactor stops using `?` against those error types in the
    /// production paths these tests instrument.
    #[test]
    fn from_impls_wrap_into_correct_variant() {
        let invalid_utf8: Vec<u8> = vec![0xff];
        let utf8_err = match std::str::from_utf8(&invalid_utf8) {
            Ok(_) => unreachable!("invalid_utf8 is never valid UTF-8"),
            Err(e) => e,
        };
        let wrapped: ModuleInfoError = utf8_err.into();
        assert!(matches!(wrapped, ModuleInfoError::Utf8Error(_)));

        let io_err = std::io::Error::other("x");
        let wrapped: ModuleInfoError = io_err.into();
        assert!(matches!(wrapped, ModuleInfoError::IoError(_)));

        // VarError uses the catch-all `Other` arm, not a dedicated
        // variant. Pin that contract so a refactor doesn't accidentally
        // promote it to its own variant without updating callers.
        let var_err = std::env::VarError::NotPresent;
        let wrapped: ModuleInfoError = var_err.into();
        assert!(matches!(wrapped, ModuleInfoError::Other(_)));
    }

    /// On Linux, `toml::de::Error` and `serde_json::Error` also have
    /// `From` impls (used by `Cargo.toml` parsing and JSON validation).
    /// Cover those arms too. Gated on Linux because the impls are
    /// `#[cfg(target_os = "linux")]`.
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_from_impls_wrap_into_other() {
        let toml_err = toml::from_str::<toml::Value>("not [valid").unwrap_err();
        let wrapped: ModuleInfoError = toml_err.into();
        assert!(matches!(wrapped, ModuleInfoError::Other(_)));

        let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let wrapped: ModuleInfoError = json_err.into();
        assert!(matches!(wrapped, ModuleInfoError::Other(_)));
    }
}
