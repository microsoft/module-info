// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Module containing the enum definition for module info field types
//!
//! This module provides a type-safe way to access module info fields
//! through the `get_module_info!` macro.

/// Represents the available module info fields
///
/// This enum provides a type-safe way to specify which module info field
/// to access when using the `get_module_info!` macro.
///
/// # Non-exhaustive
///
/// This enum is marked `#[non_exhaustive]`: additional fields may be added
/// in future minor releases without breaking SemVer. Any external `match`
/// on `ModuleInfoField` **must** include a wildcard arm (`_ => ...`) so
/// downstream code keeps compiling when new variants are introduced.
///
/// # Example
///
/// `ModuleInfoField` does not need to be imported when it only appears
/// inside `get_module_info!(ModuleInfoField::…)`: the macro pattern-matches
/// the variant as tokens, so the bare macro + result import is enough:
///
/// ```rust
/// use module_info::{get_module_info, ModuleInfoResult};
///
/// fn get_binary_name() -> ModuleInfoResult<String> {
///     let binary_name = get_module_info!(ModuleInfoField::Binary)?;
///     Ok(binary_name)
/// }
/// ```
///
/// You only need `use module_info::ModuleInfoField;` when you reference the
/// enum outside the macro (e.g. in your own `match` or when passing a value
/// into `ModuleInfoField::to_symbol_name`).
///
/// # Adding a new variant
///
/// Adding a variant requires synchronized updates in seven places across four
/// files. The enum `#[non_exhaustive]` + exhaustive matches in
/// `field_value`/`to_symbol_name`/`to_key` catch most drift as compile errors,
/// but the `get_module_info!` macro rules are token-matched and their drift
/// only surfaces at the *consumer's* call site. Skim this list:
///
/// 1. This enum declaration (add the variant)
/// 2. [`ModuleInfoField::to_symbol_name`] match arm (compile-error on miss)
/// 3. [`ModuleInfoField::to_key`] match arm (compile-error on miss)
/// 4. [`ModuleInfoField::ALL`] + `EXPECTED_VARIANT_COUNT` in the drift-guard
///    test (runtime failure on miss)
/// 5. `PackageMetadata` field + `field_value` match arm (compile-error on miss)
/// 6. `src/macros.rs`: per-variant rule in the Linux `get_module_info!`
///    macro AND in the non-Linux fallback macro. **Missing a rule here is
///    silent: `get_module_info!(ModuleInfoField::NewField)` only fails at
///    the consumer's call site.**
/// 7. `src/macros.rs`: an `@__add_to_map` line in the no-arg form of the
///    Linux `get_module_info!` macro. **Missing this line silently drops
///    the field from the no-arg HashMap.**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ModuleInfoField {
    /// The binary name
    Binary,
    /// The version of the binary
    Version,
    /// The version of the module
    ModuleVersion,
    /// The maintainer of the binary
    Maintainer,
    /// The name of the module
    Name,
    /// The type of module
    Type,
    /// The repository URL
    Repo,
    /// The branch name
    Branch,
    /// The commit hash
    Hash,
    /// The copyright information
    Copyright,
    /// The operating system information
    Os,
    /// The operating system version
    OsVersion,
}

impl ModuleInfoField {
    /// Converts the enum variant to the corresponding linker symbol name
    /// (for example, `ModuleInfoField::Binary` → `"module_info_binary"`).
    ///
    /// This is primarily an internal helper used by the `get_module_info!`
    /// macro expansion and by debugging utilities; most consumers should
    /// reach for the macro rather than calling this directly.
    ///
    /// # Returns
    /// A string slice containing the symbol name for this field.
    pub fn to_symbol_name(&self) -> &'static str {
        match self {
            ModuleInfoField::Binary => "module_info_binary",
            ModuleInfoField::Version => "module_info_version",
            ModuleInfoField::ModuleVersion => "module_info_moduleVersion",
            ModuleInfoField::Maintainer => "module_info_maintainer",
            ModuleInfoField::Name => "module_info_name",
            ModuleInfoField::Type => "module_info_type",
            ModuleInfoField::Repo => "module_info_repo",
            ModuleInfoField::Branch => "module_info_branch",
            ModuleInfoField::Hash => "module_info_hash",
            ModuleInfoField::Copyright => "module_info_copyright",
            ModuleInfoField::Os => "module_info_os",
            ModuleInfoField::OsVersion => "module_info_osVersion",
        }
    }

    /// Converts the enum variant to the JSON/`HashMap` key used for this field
    /// (for example, `ModuleInfoField::ModuleVersion` → `"moduleVersion"`).
    ///
    /// This is primarily an internal helper used by the no-argument form of
    /// `get_module_info!()` when it assembles the result `HashMap`. Direct
    /// consumer use is uncommon; reach for the macro instead.
    ///
    /// # Returns
    /// A string slice containing the key for this field in the HashMap.
    pub fn to_key(&self) -> &'static str {
        match self {
            ModuleInfoField::Binary => "binary",
            ModuleInfoField::Version => "version",
            ModuleInfoField::ModuleVersion => "moduleVersion",
            ModuleInfoField::Maintainer => "maintainer",
            ModuleInfoField::Name => "name",
            ModuleInfoField::Type => "type",
            ModuleInfoField::Repo => "repo",
            ModuleInfoField::Branch => "branch",
            ModuleInfoField::Hash => "hash",
            ModuleInfoField::Copyright => "copyright",
            ModuleInfoField::Os => "os",
            ModuleInfoField::OsVersion => "osVersion",
        }
    }

    /// All variants of `ModuleInfoField`, in a stable declaration order.
    ///
    /// This single source-of-truth list is used by [`ModuleInfoField::count`]
    /// and by the no-argument form of the `get_module_info!` macro to build
    /// the result `HashMap`. Adding a variant above and forgetting to add it
    /// here will be caught by the agreement test in `lib.rs`.
    pub const ALL: &'static [ModuleInfoField] = &[
        ModuleInfoField::Binary,
        ModuleInfoField::Version,
        ModuleInfoField::ModuleVersion,
        ModuleInfoField::Maintainer,
        ModuleInfoField::Name,
        ModuleInfoField::Type,
        ModuleInfoField::Repo,
        ModuleInfoField::Branch,
        ModuleInfoField::Hash,
        ModuleInfoField::Copyright,
        ModuleInfoField::Os,
        ModuleInfoField::OsVersion,
    ];

    /// Returns the number of variants in the `ModuleInfoField` enum.
    ///
    /// Derived from [`ModuleInfoField::ALL`] so it stays in sync automatically
    /// when variants are added or removed.
    pub const fn count() -> usize {
        Self::ALL.len()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn count_matches_all_length() {
        assert_eq!(ModuleInfoField::count(), ModuleInfoField::ALL.len());
    }

    #[test]
    fn all_has_unique_symbol_names_and_keys() {
        let symbols: HashSet<&str> = ModuleInfoField::ALL
            .iter()
            .map(|f| f.to_symbol_name())
            .collect();
        assert_eq!(
            symbols.len(),
            ModuleInfoField::ALL.len(),
            "ModuleInfoField::ALL has duplicate entries (by symbol name)"
        );

        let keys: HashSet<&str> = ModuleInfoField::ALL.iter().map(|f| f.to_key()).collect();
        assert_eq!(
            keys.len(),
            ModuleInfoField::ALL.len(),
            "ModuleInfoField::ALL has duplicate entries (by HashMap key)"
        );
    }

    #[test]
    fn every_variant_is_listed_in_all() {
        // Compile-time drift guard. This test protects the invariant
        // `ModuleInfoField::ALL` contains every variant of `ModuleInfoField`,
        // which several call sites (the `get_module_info!` macro, the
        // `print_module_info` helper, and `count()`) rely on.
        //
        // The guard has two pieces:
        //
        //   1. An exhaustive match inside the crate (where
        //      `#[non_exhaustive]` does not apply); adding a new variant
        //      without a new arm is a compile error.
        //   2. A hard-coded length assertion against `ALL`; adding a new
        //      arm above without extending `ALL` fails this test at runtime
        //      and the failure message tells the author exactly what to do.
        //
        // The two together close the loop: the first catches enum→match
        // drift, the second catches match→ALL drift.
        const EXPECTED_VARIANT_COUNT: usize = 12;

        fn canonical_key(f: ModuleInfoField) -> &'static str {
            match f {
                ModuleInfoField::Binary => "binary",
                ModuleInfoField::Version => "version",
                ModuleInfoField::ModuleVersion => "moduleVersion",
                ModuleInfoField::Maintainer => "maintainer",
                ModuleInfoField::Name => "name",
                ModuleInfoField::Type => "type",
                ModuleInfoField::Repo => "repo",
                ModuleInfoField::Branch => "branch",
                ModuleInfoField::Hash => "hash",
                ModuleInfoField::Copyright => "copyright",
                ModuleInfoField::Os => "os",
                ModuleInfoField::OsVersion => "osVersion",
            }
        }

        assert_eq!(
            ModuleInfoField::ALL.len(),
            EXPECTED_VARIANT_COUNT,
            "ModuleInfoField::ALL length changed: if you added a variant, \
             extend ALL and bump EXPECTED_VARIANT_COUNT; if you removed one, \
             drop it from ALL and bump EXPECTED_VARIANT_COUNT down"
        );

        // Every entry in `ALL` must hit a match arm above (dead entries
        // would show up as a `canonical_key` returning the wrong string).
        for f in ModuleInfoField::ALL {
            assert_eq!(f.to_key(), canonical_key(*f));
        }
    }
}
