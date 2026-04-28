// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

// Build-time diagnostics emitted via `cargo:warning=…` (the only build-script
// channel cargo surfaces by default). The leading `\x1b[2K\r` overwrites
// cargo's `warning: <crate>@<ver>:` prefix in a TTY; in non-TTY log
// collectors the escape is inert and the prefix shows through.
//
// `note!` is exported for consumers' build scripts. `error!` and `warn!`
// stay crate-private to avoid colliding with `log::error!` / `log::warn!`.

/// Emit a styled `Info:` line on cargo's build-script output channel.
///
/// Use from `build.rs` to add log lines that match the styling
/// [`generate_project_metadata_and_linker_script`](crate::generate_project_metadata_and_linker_script)
/// emits for its metadata dump. On non-Linux targets the macro is a no-op.
#[cfg(target_os = "linux")]
#[macro_export]
macro_rules! note {
    () => {
        ::std::println!("cargo:warning=\x1b[2K\r");
    };
    ($($arg:tt)+) => {
        ::std::println!("cargo:warning=\x1b[2K\r   \x1b[1m\x1b[36mInfo:\x1b[0m {}", ::std::format!($($arg)+))
    }
}

/// Non-Linux stub of [`note!`](note) so cross-platform `build.rs` compiles
/// without `#[cfg]` guards.
#[cfg(not(target_os = "linux"))]
#[macro_export]
macro_rules! note {
    () => {};
    ($($arg:tt)+) => {};
}

/// Macro for printing error messages during build
#[cfg(target_os = "linux")]
macro_rules! error {
    ($($arg:tt)+) => {
        ::std::println!("cargo:warning=\x1b[2K\r   \x1b[1m\x1b[31mError:\x1b[0m {}", ::std::format!($($arg)+))
    }
}

/// Macro for printing warning messages during build
#[cfg(target_os = "linux")]
macro_rules! warn {
    ($($arg:tt)*) => {{
        ::std::println!("cargo:warning=\x1b[2K\r   \x1b[1m\x1b[33mWarning:\x1b[0m {}", ::std::format!($($arg)*))
    }};
}

/// A macro that conditionally prints debug messages to cargo output.
///
/// This macro checks the environment variable `MODULE_INFO_DEBUG` to determine
/// whether to print debug messages. The check is performed once and the result is
/// cached using an atomic variable.
///
/// # Behavior
///
/// - First call: Checks `MODULE_INFO_DEBUG` environment variable
/// - If `MODULE_INFO_DEBUG=true`: Prints formatted message to cargo output with purple "Debug:" prefix
/// - If `MODULE_INFO_DEBUG` is unset or not "true": Suppresses output
///
/// # Implementation Details
///
/// The macro uses an atomic variable to cache the debug state:
/// - 0: Not yet checked
/// - 1: Debugging enabled
/// - 2: Debugging disabled
///
/// The `static` is declared *inside* the macro body, which means each call
/// site gets its own `DEBUG_STATE`. That's intentional: there is no public
/// crate-level "is debug on?" flag, and keeping the state adjacent to its
/// single consumer avoids a hidden global. The per-site cost is a single
/// env-var read the first time that exact `debug!(...)` expands on the
/// executing build (amortized across the life of the build script).
#[cfg(target_os = "linux")]
macro_rules! debug {
    ($($arg:tt)*) => {{
        static DEBUG_STATE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0); // 0=unchecked, 1=enabled, 2=disabled

        if DEBUG_STATE.load(std::sync::atomic::Ordering::Relaxed) == 0 {
            let val = ::std::env::var("MODULE_INFO_DEBUG")
                .map(|v| v.to_lowercase() == "true")
                .unwrap_or(false);
            DEBUG_STATE.store(if val { 1 } else { 2 }, std::sync::atomic::Ordering::Relaxed);
        }

        if DEBUG_STATE.load(std::sync::atomic::Ordering::Relaxed) == 1 {
            ::std::println!("cargo:warning=\x1b[2K\r   \x1b[1m\x1b[35mDebug:\x1b[0m {}", ::std::format!($($arg)*));
        }
    }};
}

/// Read the embedded metadata at runtime.
///
/// Two call shapes:
///
/// - `get_module_info!()` returns
///   `ModuleInfoResult<HashMap<String, String>>` covering every field that
///   resolved. Fields disabled at build time appear with value `""`.
///   Fields whose symbols failed to resolve (`.note.package` stripped from
///   the binary, or no `module_info::embed!()` invocation reached the
///   linker) are **omitted** from the map, so check `contains_key` before
///   assuming a key is present.
/// - `get_module_info!(ModuleInfoField::Binary)` returns
///   `ModuleInfoResult<String>` for a single field. Disabled fields
///   yield `Ok("")`; unresolved symbols yield
///   `Err(ModuleInfoError::NotAvailable)`.
///
/// If the process has already crashed, the same data is reachable via
/// `coredumpctl info`, `readelf -n`, or WinDbg against the core dump,
/// without going through this macro.
///
/// # Examples
///
/// Retrieve all module info:
/// ```rust
/// use module_info::{get_module_info, ModuleInfoResult};
///
/// fn print_all() -> ModuleInfoResult<()> {
///     for (key, value) in get_module_info!()? {
///         println!("{key}: {value}");
///     }
///     Ok(())
/// }
/// ```
///
/// ```rust
/// use module_info::{get_module_info, ModuleInfoResult};
///
/// fn binary_name() -> ModuleInfoResult<String> {
///     get_module_info!(ModuleInfoField::Binary)
/// }
/// ```
///
/// `ModuleInfoField::Foo` is matched as tokens by the macro, so the enum
/// itself does not need to be imported when it appears only inside
/// `get_module_info!(...)`.
///
/// # Safety
///
/// The macro reads `extern "C" static: u8` symbols emitted by the build
/// script's linker script. The symbols are placed inside the read-only
/// `.note.package` payload by the linker; the macro takes their address
/// and hands it to [`crate::extract_module_info`], which scans for the
/// terminating NUL. Memory is never written, and the bound check inside
/// `extract_module_info` keeps a stripped or corrupted section from
/// reading past the cap.
///
/// # Availability
///
/// This form is active when `embed-module-info` is enabled and the target
/// is Linux. The fallback form (declared below) is active otherwise and
/// returns `ModuleInfoError::NotAvailable` for every variant.
#[cfg(all(feature = "embed-module-info", target_os = "linux"))]
#[macro_export]
macro_rules! get_module_info {
    // Internal macro for processing a single field.
    //
    // Declares exactly the one extern static this invocation will read, then
    // hands its address to `extract_module_info`. Each symbol is typed as a
    // single `u8`; the linker script places it at a specific byte inside the
    // `.note.package` payload, so the symbol's "value" is really its *address*.
    // Typing it as a sized array (`[u8; 255]`) would be a lie: the symbol is
    // not a standalone 255-byte object, it's a pointer into a shared JSON blob.
    // Opaque `u8` is the smallest honest type and matches how the macro uses
    // it (`&$symbol as *const u8`). Declaring only the single symbol per
    // invocation keeps clippy's `unused_extern_items` lint quiet when the macro
    // is used many times in one function.
    (@__extract $symbol:ident) => {{
        extern "C" {
            #[allow(non_upper_case_globals)]
            static $symbol: u8;
        }
        unsafe { $crate::extract_module_info(&$symbol as *const u8) }
    }};

    // Internal macro for adding a field to the accumulating HashMap.
    //
    // Insert every successfully-read field into the map, including empty
    // strings. The embedded JSON always carries every key (the
    // `.note.package` layout is fixed at build time), so an empty value
    // encodes the documented "disabled at build time" state; consumers
    // rely on `map.contains_key("repo")` returning true regardless of
    // whether the embedder deliberately left `repo` empty. Filtering out
    // empty values here would diverge from the single-field form of the
    // macro (which returns `Ok("")` for the same bytes) and would force
    // callers to reach for `map.get("repo").cloned().unwrap_or_default()`
    // to reimplement the documented contract.
    //
    // A read failure (e.g. the symbol resolved to a null pointer on a
    // binary where the note section was stripped) is still skipped;
    // `print_module_info`'s guardrail above treats "fewer than the required
    // identity fields populated" as `NotAvailable`.
    (@__add_to_map $info_map:ident, $symbol:ident, $key:literal) => {{
        if let Ok(value) = get_module_info!(@__extract $symbol) {
            $info_map.insert($key.to_string(), value);
        }
    }};

    // Public rules: one per `ModuleInfoField` variant. Dispatch happens at
    // macro-expansion time, so a typo like `ModuleInfoField::Foo` is a
    // compile error (no matching rule) rather than a runtime error.
    // `ModuleInfoField` is `#[non_exhaustive]`, so adding a variant requires
    // adding a rule here, which keeps the macro and the enum in lock-step.
    (ModuleInfoField::Binary) => { get_module_info!(@__extract module_info_binary) };
    (ModuleInfoField::Version) => { get_module_info!(@__extract module_info_version) };
    (ModuleInfoField::ModuleVersion) => { get_module_info!(@__extract module_info_moduleVersion) };
    (ModuleInfoField::Maintainer) => { get_module_info!(@__extract module_info_maintainer) };
    (ModuleInfoField::Name) => { get_module_info!(@__extract module_info_name) };
    (ModuleInfoField::Type) => { get_module_info!(@__extract module_info_type) };
    (ModuleInfoField::Repo) => { get_module_info!(@__extract module_info_repo) };
    (ModuleInfoField::Branch) => { get_module_info!(@__extract module_info_branch) };
    (ModuleInfoField::Hash) => { get_module_info!(@__extract module_info_hash) };
    (ModuleInfoField::Copyright) => { get_module_info!(@__extract module_info_copyright) };
    (ModuleInfoField::Os) => { get_module_info!(@__extract module_info_os) };
    (ModuleInfoField::OsVersion) => { get_module_info!(@__extract module_info_osVersion) };

    // Public rule: handles requests for all module info fields
    () => {{
        // Fully-qualified path: do NOT `use std::collections::HashMap;`.
        // This macro is invoked in arbitrary call sites; a local `use`
        // would introduce `HashMap` into the caller's scope and could
        // shadow or conflict with their existing imports.
        // Pre-allocate with exact capacity
        let mut info_map: ::std::collections::HashMap<::std::string::String, ::std::string::String> =
            ::std::collections::HashMap::with_capacity($crate::ModuleInfoField::count());

        // Each @__add_to_map call expands @__extract, which declares its own
        // single-symbol extern block; no shared declaration needed here.
        get_module_info!(@__add_to_map info_map, module_info_binary, "binary");
        get_module_info!(@__add_to_map info_map, module_info_version, "version");
        get_module_info!(@__add_to_map info_map, module_info_moduleVersion, "moduleVersion");
        get_module_info!(@__add_to_map info_map, module_info_maintainer, "maintainer");
        get_module_info!(@__add_to_map info_map, module_info_name, "name");
        get_module_info!(@__add_to_map info_map, module_info_type, "type");
        get_module_info!(@__add_to_map info_map, module_info_repo, "repo");
        get_module_info!(@__add_to_map info_map, module_info_branch, "branch");
        get_module_info!(@__add_to_map info_map, module_info_hash, "hash");
        get_module_info!(@__add_to_map info_map, module_info_copyright, "copyright");
        get_module_info!(@__add_to_map info_map, module_info_os, "os");
        get_module_info!(@__add_to_map info_map, module_info_osVersion, "osVersion");

        $crate::ModuleInfoResult::<::std::collections::HashMap<::std::string::String, ::std::string::String>>::Ok(info_map)
    }};
}

/// No-op version of get_module_info macro for non-Linux platforms
///
/// This ensures that code can still be compiled on non-Linux platforms without errors,
/// but returns `NotAvailable` at runtime. The per-variant rules below must
/// mirror the Linux-side set exactly; an unknown variant name must be a
/// compile error on every target, not "compiles on Windows, fails on Linux."
#[cfg(any(not(feature = "embed-module-info"), not(target_os = "linux")))]
#[macro_export]
macro_rules! get_module_info {
    // One rule per known variant, keyed off a literal identifier (not
    // `$field:ident`), so a typo like `ModuleInfoField::Foo` is a compile
    // error here the same way it is on Linux.
    (ModuleInfoField::Binary) => { $crate::__module_info_not_available!("Binary") };
    (ModuleInfoField::Version) => { $crate::__module_info_not_available!("Version") };
    (ModuleInfoField::ModuleVersion) => { $crate::__module_info_not_available!("ModuleVersion") };
    (ModuleInfoField::Maintainer) => { $crate::__module_info_not_available!("Maintainer") };
    (ModuleInfoField::Name) => { $crate::__module_info_not_available!("Name") };
    (ModuleInfoField::Type) => { $crate::__module_info_not_available!("Type") };
    (ModuleInfoField::Repo) => { $crate::__module_info_not_available!("Repo") };
    (ModuleInfoField::Branch) => { $crate::__module_info_not_available!("Branch") };
    (ModuleInfoField::Hash) => { $crate::__module_info_not_available!("Hash") };
    (ModuleInfoField::Copyright) => { $crate::__module_info_not_available!("Copyright") };
    (ModuleInfoField::Os) => { $crate::__module_info_not_available!("Os") };
    (ModuleInfoField::OsVersion) => { $crate::__module_info_not_available!("OsVersion") };

    // Handle the empty form that returns all fields
    () => {{
        $crate::ModuleInfoResult::<::std::collections::HashMap<::std::string::String, ::std::string::String>>::Err(
            $crate::ModuleInfoError::NotAvailable(
                "Module info is only available on Linux platforms with embed-module-info feature enabled.".to_string(),
            ),
        )
    }};
}

/// Internal helper: builds the `Err(NotAvailable(..))` result the per-variant
/// rules of the non-Linux `get_module_info!` return. Kept private (leading
/// `__`) because it's an implementation detail of the macro.
#[cfg(any(not(feature = "embed-module-info"), not(target_os = "linux")))]
#[doc(hidden)]
#[macro_export]
macro_rules! __module_info_not_available {
    ($field:literal) => {{
        $crate::ModuleInfoResult::<String>::Err($crate::ModuleInfoError::NotAvailable(format!(
            "Module info field '{}' is only available on Linux platforms with embed-module-info feature enabled.",
            $field
        )))
    }};
}
