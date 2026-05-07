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
