// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Std-only ELF parser sufficient to answer "is this binary's
//! `.note.package` section SHT_NOTE-typed, and what bytes does it
//! contain?". Shared between the per-example integration tests via
//! `#[path = "../../_test_support/note_section.rs"] mod note_section;`
//! so each test stays a single self-contained file but the parser logic
//! lives in one place.
//!
//! Using a small in-process parser instead of shelling out to `readelf`:
//! - Keeps tests independent of binutils being on the runner / dev box.
//! - Returns structured results (section type, offset, size, payload
//!   slice) that tests assert on directly, instead of stringy regex
//!   matching against `readelf -SW` text columns.
//! - Avoids the `readelf -p` truncation footgun on binutils ≤ 2.38 that
//!   silently drops the tail of long `.note.package` payloads.
//!
//! Scope is deliberately small: we don't decode note headers, parse
//! relocations, follow segment maps, or care about anything outside
//! the section header table and the section name string table. The
//! crate's stated supported targets (x86_64 / aarch64 / i686 Linux,
//! all little-endian) cover both ELF32 and ELF64 with little-endian
//! byte order, which is what this parser handles. A big-endian ELF or
//! a malformed header returns `Err(...)` with a diagnostic message
//! rather than panicking on slice indexing.

#![allow(dead_code)] // Each consuming test uses a subset of the API.

use std::fs;
use std::path::Path;

/// SHT_NOTE per System V gABI §5.2 (`Elf{32,64}_Shdr.sh_type`). The crate
/// emits `.note.package` with this type via the linker script's
/// `KEEP(*(.note.package))` clause picking up the rlib's
/// `#[link_section = ".note.package"]` static. If the rlib is dropped
/// from the final link, ld synthesizes the section from `BYTE(...)`
/// directives alone and the type degrades to `SHT_PROGBITS = 1`, which
/// crash-triage tools filter out silently.
pub const SHT_NOTE: u32 = 7;
pub const SHT_PROGBITS: u32 = 1;

/// Information about a `.note.package` section recovered from an ELF
/// artifact on disk. Tests use it for the section-type guard
/// (`sh_type == SHT_NOTE`), the note-owner check (must be `"FDO"` for
/// the systemd package-metadata format), and content checks against
/// the descriptor (the embedded JSON must carry the expected `binary`
/// field).
pub struct NotePackage {
    /// `sh_type` from the section header. `SHT_NOTE` (= 7) is the value
    /// the crash-triage ecosystem expects; `SHT_PROGBITS` (= 1) means
    /// the rlib's `.note.package` input section never reached the link.
    pub sh_type: u32,
    /// Note `n_type` (vendor-specific). Should equal `0xcafe1a7e` (the
    /// crate's `N_TYPE` constant) on a well-formed embed.
    pub n_type: u32,
    /// Note name (the "owner"). Should be `"FDO"` for FDO-spec
    /// package-metadata notes.
    pub owner: String,
    /// Note descriptor: the embedded JSON metadata, with any
    /// alignment-padding NULs at the tail trimmed off.
    pub descriptor: Vec<u8>,
}

impl NotePackage {
    /// Returns the descriptor as a UTF-8 string. The crate sanitizes
    /// embedded metadata to ASCII at build time, so this never fails on
    /// a well-formed binary; if it does, the section was corrupted or
    /// the test is reading the wrong file.
    pub fn descriptor_as_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.descriptor)
    }
}

/// Locate `.note.package` in `path` and return its section header type
/// plus the section payload bytes. Returns `Err` for an unreadable
/// file, a malformed ELF, an unsupported byte order, or a missing
/// section. Only LE32/LE64 ELFs are supported; the crate is gated on
/// little-endian Linux targets so this matches the build matrix.
pub fn read_note_package(path: &Path) -> Result<NotePackage, String> {
    let bytes = fs::read(path).map_err(|e| format!("read {} failed: {e}", path.display()))?;

    // ELF magic + class + data byte at fixed offsets in the ident array.
    if bytes.len() < 0x40 {
        return Err(format!(
            "{} is too small to be ELF ({} bytes)",
            path.display(),
            bytes.len()
        ));
    }
    if bytes.get(..4) != Some(b"\x7fELF") {
        return Err(format!("{} is not an ELF file", path.display()));
    }

    // EI_CLASS at offset 4: 1 = ELF32, 2 = ELF64. EI_DATA at offset 5:
    // 1 = little-endian, 2 = big-endian. The crate's supported targets
    // are all little-endian; reject big-endian explicitly so a future
    // big-endian regression surfaces here instead of via subtle
    // byte-order mismatches.
    let ei_class = bytes[4];
    let ei_data = bytes[5];
    if ei_data != 1 {
        return Err(format!(
            "{} is big-endian; module-info only supports little-endian targets",
            path.display()
        ));
    }

    let (e_shoff, e_shentsize, e_shnum, e_shstrndx) = match ei_class {
        // ELF32 header layout (System V gABI Figure 4-2):
        //   e_shoff    @ 0x20 (4 bytes)
        //   e_shentsize @ 0x2E (2 bytes)
        //   e_shnum    @ 0x30 (2 bytes)
        //   e_shstrndx @ 0x32 (2 bytes)
        1 => (
            u32_le(&bytes, 0x20)? as u64,
            u16_le(&bytes, 0x2E)? as usize,
            u16_le(&bytes, 0x30)? as usize,
            u16_le(&bytes, 0x32)? as usize,
        ),
        // ELF64 header layout (System V gABI Figure 4-3):
        //   e_shoff    @ 0x28 (8 bytes)
        //   e_shentsize @ 0x3A (2 bytes)
        //   e_shnum    @ 0x3C (2 bytes)
        //   e_shstrndx @ 0x3E (2 bytes)
        2 => (
            u64_le(&bytes, 0x28)?,
            u16_le(&bytes, 0x3A)? as usize,
            u16_le(&bytes, 0x3C)? as usize,
            u16_le(&bytes, 0x3E)? as usize,
        ),
        other => {
            return Err(format!(
                "{} has unknown EI_CLASS {} (expected 1 or 2)",
                path.display(),
                other
            ))
        }
    };

    if e_shentsize == 0 || e_shnum == 0 {
        return Err(format!(
            "{} has no section headers (shnum={e_shnum}, shentsize={e_shentsize})",
            path.display()
        ));
    }
    if e_shstrndx >= e_shnum {
        return Err(format!(
            "{} e_shstrndx {} out of range (shnum={e_shnum})",
            path.display(),
            e_shstrndx
        ));
    }

    let shoff = usize::try_from(e_shoff)
        .map_err(|_| format!("{} e_shoff {} overflows usize", path.display(), e_shoff))?;
    let table_end = shoff
        .checked_add(e_shnum.checked_mul(e_shentsize).ok_or_else(|| {
            format!(
                "{} section header table size overflow (shnum={e_shnum} * shentsize={e_shentsize})",
                path.display()
            )
        })?)
        .ok_or_else(|| format!("{} section header table end overflow", path.display()))?;
    if table_end > bytes.len() {
        return Err(format!(
            "{} section header table extends past EOF ({} > {})",
            path.display(),
            table_end,
            bytes.len()
        ));
    }

    // Find shstrtab so section names can be resolved.
    let (shstrtab_off, shstrtab_size) =
        section_off_size(&bytes, ei_class, shoff, e_shentsize, e_shstrndx)?;
    let shstrtab = bytes
        .get(shstrtab_off..shstrtab_off.saturating_add(shstrtab_size))
        .ok_or_else(|| format!("{} shstrtab out of range", path.display()))?;

    // Walk every section header looking for `.note.package`.
    for i in 0..e_shnum {
        let hdr_off = shoff + i * e_shentsize;
        let sh_name = u32_le(&bytes, hdr_off)? as usize;
        let sh_type = u32_le(&bytes, hdr_off + 4)?;
        let name = read_cstr(shstrtab, sh_name).unwrap_or("");
        if name == ".note.package" {
            let (off, size) = section_off_size(&bytes, ei_class, shoff, e_shentsize, i)?;
            let payload = bytes.get(off..off.saturating_add(size)).ok_or_else(|| {
                format!(
                    "{} .note.package payload [{off}..{}] is out of range",
                    path.display(),
                    off + size
                )
            })?;
            let parsed = parse_note_payload(payload).map_err(|e| {
                format!("{} .note.package payload is malformed: {e}", path.display())
            })?;
            return Ok(NotePackage {
                sh_type,
                n_type: parsed.n_type,
                owner: parsed.owner,
                descriptor: parsed.descriptor,
            });
        }
    }

    Err(format!("{} has no .note.package section", path.display()))
}

/// Note-header parse result: just enough fields for the tests.
struct ParsedNote {
    n_type: u32,
    owner: String,
    descriptor: Vec<u8>,
}

/// Decode a single ELF note (`Elf{32,64}_Nhdr` + name + desc) from the
/// section payload. The note layout is identical for ELF32 and ELF64
/// per the System V gABI (three little-endian u32 fields, no
/// class-specific widening), so one parser handles both.
fn parse_note_payload(payload: &[u8]) -> Result<ParsedNote, String> {
    // Header: n_namesz (4) | n_descsz (4) | n_type (4).
    let n_namesz = u32_le(payload, 0)? as usize;
    let n_descsz = u32_le(payload, 4)? as usize;
    let n_type = u32_le(payload, 8)?;

    // Name and desc are 4-byte aligned per gABI §5.2 ("Note Section").
    // The crate uses `NOTE_ALIGN = 4` everywhere; round up offsets to
    // match what `module-info`'s linker script emits.
    let name_start = 12usize;
    let name_end = name_start
        .checked_add(n_namesz)
        .ok_or_else(|| format!("name end overflow (n_namesz={n_namesz})"))?;
    if payload.len() < name_end {
        return Err(format!(
            "note name extends past payload ({name_end} > {})",
            payload.len()
        ));
    }
    // Owner is NUL-terminated inside `n_namesz`; strip the trailing NUL
    // before lossy-decoding so consumers see "FDO" not "FDO\0".
    // `split(...).next()` always yields at least the empty prefix, so
    // `.unwrap_or(&[])` is just a defensive default.
    let owner_bytes = payload[name_start..name_end]
        .split(|&b| b == 0)
        .next()
        .unwrap_or(&[]);
    let owner = String::from_utf8_lossy(owner_bytes).into_owned();

    let desc_start = align_up(name_end, 4);
    let desc_end = desc_start
        .checked_add(n_descsz)
        .ok_or_else(|| format!("desc end overflow (n_descsz={n_descsz})"))?;
    if payload.len() < desc_end {
        return Err(format!(
            "note desc extends past payload ({desc_end} > {})",
            payload.len()
        ));
    }
    // Strip trailing NUL padding the crate appends to the descriptor
    // (1..=NOTE_ALIGN bytes, see `metadata::render_note_payloads`); the
    // JSON itself never ends in NUL.
    let mut descriptor = payload[desc_start..desc_end].to_vec();
    while descriptor.last().copied() == Some(0) {
        descriptor.pop();
    }

    Ok(ParsedNote {
        n_type,
        owner,
        descriptor,
    })
}

/// Round `n` up to the next multiple of `align`. `align` is always 4
/// here (ELF note alignment); guard the math so an overflow surfaces as
/// an error instead of silently wrapping.
fn align_up(n: usize, align: usize) -> usize {
    let mask = align - 1;
    (n.saturating_add(mask)) & !mask
}

/// Read `(sh_offset, sh_size)` for the section at index `idx`. Both fields
/// are 32-bit on ELF32 and 64-bit on ELF64; `usize` is enough to address
/// any binary that fits in memory on the host.
fn section_off_size(
    bytes: &[u8],
    ei_class: u8,
    shoff: usize,
    shentsize: usize,
    idx: usize,
) -> Result<(usize, usize), String> {
    let hdr_off = shoff + idx * shentsize;
    match ei_class {
        // Elf32_Shdr layout: sh_name(4) sh_type(4) sh_flags(4) sh_addr(4)
        //   sh_offset(4) sh_size(4) sh_link(4) sh_info(4) sh_addralign(4) sh_entsize(4)
        // sh_offset @ +0x10, sh_size @ +0x14
        1 => Ok((
            u32_le(bytes, hdr_off + 0x10)? as usize,
            u32_le(bytes, hdr_off + 0x14)? as usize,
        )),
        // Elf64_Shdr layout: sh_name(4) sh_type(4) sh_flags(8) sh_addr(8)
        //   sh_offset(8) sh_size(8) sh_link(4) sh_info(4) sh_addralign(8) sh_entsize(8)
        // sh_offset @ +0x18, sh_size @ +0x20
        2 => Ok((
            usize::try_from(u64_le(bytes, hdr_off + 0x18)?)
                .map_err(|_| format!("section {idx} sh_offset overflows usize"))?,
            usize::try_from(u64_le(bytes, hdr_off + 0x20)?)
                .map_err(|_| format!("section {idx} sh_size overflows usize"))?,
        )),
        other => Err(format!("unknown EI_CLASS {other}")),
    }
}

fn u16_le(bytes: &[u8], off: usize) -> Result<u16, String> {
    let s = bytes
        .get(off..off + 2)
        .ok_or_else(|| format!("u16 read at {off} out of range"))?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

fn u32_le(bytes: &[u8], off: usize) -> Result<u32, String> {
    let s = bytes
        .get(off..off + 4)
        .ok_or_else(|| format!("u32 read at {off} out of range"))?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn u64_le(bytes: &[u8], off: usize) -> Result<u64, String> {
    let s = bytes
        .get(off..off + 8)
        .ok_or_else(|| format!("u64 read at {off} out of range"))?;
    Ok(u64::from_le_bytes([
        s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7],
    ]))
}

/// Read a NUL-terminated string starting at `off` in `bytes` and decode
/// it as UTF-8. Section names in shstrtab are ASCII in practice;
/// returning a `&str` rather than `&[u8]` keeps the call sites tidy.
fn read_cstr(bytes: &[u8], off: usize) -> Option<&str> {
    let tail = bytes.get(off..)?;
    let end = tail.iter().position(|&b| b == 0)?;
    std::str::from_utf8(&tail[..end]).ok()
}
