// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

// This example is Linux-specific and demonstrates embedding module info metadata
// in ELF binaries and preserving it in crash dumps.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    module_info::generate_project_metadata_and_linker_script()?;
    Ok(())
}
