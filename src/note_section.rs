// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::{fs::File, io::Write, path::Path};

use crate::{
    error::ModuleInfoResult,
    utils::{align_len, bytes_to_linker_directives},
};

/// Structure to handle ELF note section
pub struct NoteSection {
    pub note_section: Vec<u8>,
    pub linker_script: String,
}

impl NoteSection {
    /// Create a new note section
    pub fn new(
        n_type: u32,
        owner: &str,
        desc: &str,
        linker_script_desc: &str,
        align: usize,
    ) -> ModuleInfoResult<Self> {
        let owner_bytes = owner.as_bytes();
        let desc_bytes = desc.as_bytes();
        let desc_len = u32::try_from(desc_bytes.len()).map_err(|_| {
            crate::ModuleInfoError::Other(
                format!(
                    "note description exceeds u32 range: {} bytes",
                    desc_bytes.len()
                )
                .into(),
            )
        })?;

        // ELF note header per System V gABI §5.2 / `Elf32_Nhdr` in <elf.h>:
        // three little-endian u32 fields (n_namesz, n_descsz, n_type). See
        // https://refspecs.linuxfoundation.org/elf/gabi4+/ch5.pheader.html#note_section
        // The 12-byte layout is identical on ELF32 and ELF64. `to_le_bytes`
        // always emits little-endian bytes regardless of host byte order,
        // which matches the ELF convention for little-endian targets; the
        // crate does not currently support big-endian hosts (see the
        // crate-level "Limitations" section).
        let owner_len_u32 = u32::try_from(owner_bytes.len()).map_err(|_| {
            crate::ModuleInfoError::Other(
                format!("note owner exceeds u32 range: {} bytes", owner_bytes.len()).into(),
            )
        })?;
        let n_namesz = owner_len_u32.checked_add(1).ok_or_else(|| {
            crate::ModuleInfoError::Other(
                "note owner length + null terminator overflows u32".into(),
            )
        })?;
        // Hard-error rather than `debug_assert!`: a build script may run in
        // release mode (`cargo build --release`), and silently emitting a
        // malformed ELF note from a swapped argument would be worse than
        // failing the build. The only FDO-packaged owner we emit is "FDO\0"
        // (4 bytes), so n_namesz < 4 means the caller passed the wrong value.
        if n_namesz < 4 {
            return Err(crate::ModuleInfoError::Other(
                format!(
                    "n_namesz={n_namesz} looks wrong — the FDO-packaged owner name is 4 bytes (\"FDO\\0\")"
                )
                .into(),
            ));
        }

        let aligned_owner_len = align_len(n_namesz, align);
        let aligned_desc_len = align_len(desc_len, align);
        let mut header = Vec::with_capacity(12);
        header.extend_from_slice(&n_namesz.to_le_bytes());
        header.extend_from_slice(&desc_len.to_le_bytes());
        header.extend_from_slice(&n_type.to_le_bytes());

        let mut owner_block = Vec::with_capacity(aligned_owner_len as usize);
        owner_block.extend_from_slice(owner_bytes);
        owner_block.push(0);
        if owner_block.len() < aligned_owner_len as usize {
            owner_block.resize(aligned_owner_len as usize, 0);
        }

        // No NUL terminator on desc: ELF treats it as an opaque blob sized by `descsz`.
        let mut desc_block = Vec::with_capacity(aligned_desc_len as usize);
        desc_block.extend_from_slice(desc_bytes);
        if desc_block.len() < aligned_desc_len as usize {
            desc_block.resize(aligned_desc_len as usize, 0);
        }

        let expected_size = header.len() + owner_block.len() + desc_block.len();
        let mut note_section = Vec::with_capacity(expected_size);
        note_section.extend_from_slice(&header);
        note_section.extend_from_slice(&owner_block);
        note_section.extend_from_slice(&desc_block);

        let header_hex = bytes_to_linker_directives(&header);
        let owner_hex = bytes_to_linker_directives(&owner_block);
        let linker_script =
            Self::generate_linker_script(&header_hex, &owner_hex, linker_script_desc, align);

        debug!(
            "Note section built: owner={:?} n_type={:#x} desc_len={} total={}",
            String::from_utf8_lossy(owner_bytes),
            n_type,
            desc_len,
            note_section.len()
        );

        Ok(Self {
            note_section,
            linker_script,
        })
    }

    /// Generate linker script for the note section with embedded note data.
    ///
    /// `align` is propagated into the `ALIGN(..)` directive so the linker's
    /// section alignment stays in sync with the padding computed above.
    fn generate_linker_script(
        header_hex: &str,
        owner_hex: &str,
        desc_encoded: &str,
        align: usize,
    ) -> String {
        format!(
            r#"/* This linker script is auto-generated. Do not edit manually. */
/* Generated by module_info crate */
SECTIONS
{{
  .note.package : ALIGN({align}) {{
    KEEP(*(.note.package))
    /* Note section header */
{header_hex}
    /* Owner string */
{owner_hex}

    /* Description data */{desc_encoded}
  }}
}}
INSERT AFTER .note.gnu.build-id;
/* End of linker script */
"#
        )
    }

    /// Save the note section (debugging aid).
    #[must_use = "save_section returns a Result; ignoring it hides I/O errors writing the note section"]
    pub(crate) fn save_section(&self, path: &Path) -> ModuleInfoResult<()> {
        let mut file = File::create(path)?;
        file.write_all(&self.note_section)?;
        Ok(())
    }

    /// Save the linker script to `<out_dir>/linker_script.ld`.
    ///
    /// Pure file writer; the caller controls whether to emit
    /// `cargo:rustc-link-arg=-T<path>` (see
    /// [`crate::EmbedOptions::emit_cargo_link_arg`]) so static-library flows
    /// whose final link happens later can pass the script to their own linker.
    #[must_use = "save_linker_script returns the PathBuf of the written script; ignoring it also hides I/O errors"]
    pub fn save_linker_script(&self, out_dir: &Path) -> ModuleInfoResult<std::path::PathBuf> {
        debug!("OUT_DIR for linker script: {}", out_dir.display());

        let script_path = out_dir.join("linker_script.ld");
        debug!("Linker script path: {}", script_path.display());

        let mut file = File::create(&script_path)?;
        file.write_all(self.linker_script.as_bytes())?;

        let script_preview = self
            .linker_script
            .lines()
            .take(10)
            .collect::<Vec<&str>>()
            .join("\n");
        debug!("Linker script preview:\n{}", script_preview);
        debug!(
            "Linker script file ({} bytes) written to {}",
            self.linker_script.len(),
            script_path.display()
        );

        Ok(script_path)
    }
}
