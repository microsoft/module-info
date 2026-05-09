// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    module_info::generate_project_metadata_and_linker_script()?;
    Ok(())
}
