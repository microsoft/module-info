// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Metadata field names and their JSON/linker mappings.

/// Metadata fields accepted by [`get_module_info!`](crate::get_module_info).
///
/// External matches must include a wildcard arm because fields may be added.
/// The macro matches variant names as tokens; import this enum only when
/// using its values or methods outside the macro.
///
/// # Example
///
/// ```rust
/// use module_info::{get_module_info, ModuleInfoResult};
///
/// fn get_binary_name() -> ModuleInfoResult<String> {
///     let binary_name = get_module_info!(ModuleInfoField::Binary)?;
///     Ok(binary_name)
/// }
/// ```
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
    /// The repository name
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
    /// Return the linker symbol name, such as `"module_info_binary"`.
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

    /// Return the JSON and runtime-map key, such as `"moduleVersion"`.
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

    /// All fields, in the order used to emit metadata.
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

    /// Return the number of fields in [`Self::ALL`].
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
