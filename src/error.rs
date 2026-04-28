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
    /// The module info feature is not enabled or not supported on this platform
    ///
    /// This error occurs when:
    /// - The "embed-module-info" feature is not enabled in your Cargo.toml
    /// - The code is running on a non-Linux platform
    /// - The String contains a detailed error message explaining why module info is not available
    NotAvailable(String),

    /// A null pointer was encountered during metadata extraction
    ///
    /// This error occurs when attempting to extract module info from a null pointer,
    /// which might happen if the linker script failed to properly embed the metadata
    /// or if the binary was stripped of its note sections.
    NullPointer,

    /// An error occurred while parsing UTF-8 data from embedded metadata
    ///
    /// This error occurs when the binary contains module info that isn't valid UTF-8,
    /// which might happen if the binary was corrupted or if there was an issue during
    /// the build process.
    Utf8Error(std::str::Utf8Error),

    /// A malformed JSON string was encountered in the embedded metadata
    ///
    /// This error occurs when the extracted metadata string doesn't follow the expected
    /// JSON format. The String contains details about what specific formatting issue was found.
    MalformedJson(String),

    /// Error when generated metadata exceeds the maximum allowed size
    ///
    /// This occurs when the generated metadata JSON is larger than the maximum allowed size.
    /// See `MAX_JSON_SIZE` (1 KiB) for the current limit, which keeps the note section compact.
    /// The String contains a message with more details about the size limitation.
    MetadataTooLarge(String),

    /// An IO error occurred while reading or writing metadata
    ///
    /// This typically happens during build time when generating the linker script or
    /// reading from Cargo.toml. The contained error provides more details about what went wrong.
    IoError(std::io::Error),

    /// Any other errors that don't fit into the above categories
    ///
    /// This is a catch-all error for unexpected issues. The boxed error contains
    /// the original error that occurred.
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
