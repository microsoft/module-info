// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::{
    env,
    fs::File,
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
};

use toml::Value;

use crate::ModuleInfoResult;

/// Format raw bytes as `    BYTE(0xNN);` linker-script directives, one per line.
///
/// Used by both `note_section` (for the ELF note header + owner) and
/// `metadata` (for the JSON payload) to emit bytes the linker can splat
/// into the `.note.package` section.
pub(crate) fn bytes_to_linker_directives(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("    BYTE(0x{b:02X});"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Round `len` up to the given `align` boundary (e.g., 4 for ELF note sections).
///
/// # Panics
/// Debug-only: panics if `align` is not a power of two.
///
/// # Release-mode behavior
/// Release builds saturate to `u32::MAX` on overflow rather than rounding
/// *down* below `len` (which the old naive `saturating_add` + `& !(align-1)`
/// did; a saturated `u32::MAX & !3` evaluates to `0xFFFFFFFC`, violating the
/// "returned value is >= len" post-condition). Saturating to `u32::MAX` lets
/// downstream size checks (e.g. `MAX_JSON_SIZE`) notice a too-large input
/// instead of silently corrupting the `.note.package` layout.
pub(crate) fn align_len(len: u32, align: usize) -> u32 {
    debug_assert!(align.is_power_of_two(), "align must be a power of two");
    let align_u32 = u32::try_from(align).unwrap_or(u32::MAX);
    let mask = align_u32.saturating_sub(1);
    // If `len + mask` would overflow, no valid aligned value fits in u32;
    // saturate to u32::MAX so downstream size checks notice, rather than
    // silently rounding *down* below `len`.
    match len.checked_add(mask) {
        Some(sum) => sum & !mask,
        None => u32::MAX,
    }
}

/// Get the operating system distro information.
///
/// Best-effort: reads `/etc/os-release` (the modern Linux convention) and
/// returns the parsed `(ID, VERSION_ID)` tuple. When `/etc/os-release` is
/// missing or unparseable (common in stripped-down containers and minimal
/// images), falls back to `("Linux", "Unknown")` rather than erroring,
/// because the embedded metadata's main job is to survive into a crash
/// dump and a degraded value is more useful there than no value.
///
/// # Returns
/// A tuple `(OS name, OS version)`. May be `("Linux", "Unknown")` when
/// detection produces no usable result.
///
/// # Errors
/// Returns `ModuleInfoError::IoError` if `/etc/os-release` exists but I/O
/// fails while reading it, or `ModuleInfoError::Other` if the file exceeds
/// the 10 KiB safety cap. A missing file is *not* an error; see above.
#[must_use = "distro info is the sole output of this function; discarding it yields no useful side effects"]
pub fn get_distro_info() -> ModuleInfoResult<(String, String)> {
    const MAX_FILE_SIZE: usize = 10 * 1024;

    let os_release = std::fs::File::open("/etc/os-release");
    if let Err(ref e) = os_release {
        // Surface under MODULE_INFO_DEBUG=true so the Linux/Unknown fallback
        // isn't a silent mystery (common in stripped-down containers).
        debug!("get_distro_info: /etc/os-release unavailable: {}", e);
    }
    if let Ok(file) = os_release {
        // `take` caps the read at the OS boundary; prevents unbounded alloc
        // if /etc/os-release is a symlink to /dev/zero or a huge tmpfs file.
        let mut limited = file.take(MAX_FILE_SIZE as u64 + 1);
        // Lossy decode: a stray non-UTF-8 byte should fall back rather than
        // hard-error out of this best-effort helper.
        let mut bytes = Vec::new();
        let bytes_read = limited
            .read_to_end(&mut bytes)
            .map_err(crate::ModuleInfoError::IoError)?;

        if bytes_read > MAX_FILE_SIZE {
            return Err(crate::ModuleInfoError::Other(
                format!("os-release file too large: exceeds {MAX_FILE_SIZE} bytes").into(),
            ));
        }

        let content = String::from_utf8_lossy(&bytes).into_owned();

        let mut name = String::new();
        let mut version = String::new();

        for line in content.lines() {
            if line.starts_with("ID=") {
                name = line.trim_start_matches("ID=").trim_matches('"').to_string();
            } else if line.starts_with("VERSION_ID=") {
                version = line
                    .trim_start_matches("VERSION_ID=")
                    .trim_matches('"')
                    .to_string();
            }
        }

        if !name.is_empty() {
            return Ok((name, version));
        }
    }

    Ok(("Linux".to_string(), "Unknown".to_string()))
}

/// Get git repository information
///
/// Retrieves information about the current git repository including:
/// - Current branch name
/// - Commit hash
/// - Repository name (from git remote URL, or directory name as fallback)
///
/// This information is used to track the exact version of code used to build the binary.
///
/// # Returns
/// A tuple containing (branch name, commit hash, repository name).
/// Returns "unknown" for git-dependent fields if git is unavailable or fails.
#[must_use = "git info is the sole output of this function; ignoring it means the call did no useful work"]
pub fn get_git_info() -> ModuleInfoResult<(String, String, String)> {
    let project_path = get_project_path();

    // Failures (missing git, non-zero exit, non-UTF-8 output) are non-fatal;
    // caller substitutes "unknown". `stdin(Stdio::null())` guards against
    // a misconfigured credential helper prompting for input.
    let run_git = |args: &[&str]| -> Option<String> {
        match Command::new("git")
            .current_dir(&project_path)
            .args(args)
            .stdin(Stdio::null())
            .output()
        {
            Ok(output) if output.status.success() => {
                // Strict `from_utf8` (not lossy) so non-UTF-8 git output
                // surfaces as a debug log instead of producing U+FFFD
                // that `sanitize_for_linker_script` later strips into a
                // mystery short string.
                match std::str::from_utf8(&output.stdout) {
                    Ok(s) => Some(s.trim().to_string()),
                    Err(e) => {
                        debug!(
                            "git {:?} returned non-UTF-8 stdout ({e}); treating as 'unknown'",
                            args
                        );
                        None
                    }
                }
            }
            Ok(output) => {
                debug!(
                    "git {:?} exited with {}: {}",
                    args,
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                );
                None
            }
            Err(e) => {
                debug!("git {:?} failed to spawn: {}", args, e);
                None
            }
        }
    };

    let branch =
        run_git(&["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    let hash = run_git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_string());

    let repo_name = run_git(&["remote", "get-url", "origin"])
        .and_then(|url| parse_repo_name_from_url(&url))
        .or_else(|| {
            project_path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "Unknown".to_string());

    Ok((branch, hash, repo_name))
}

/// Parse repository name from a git remote URL
///
/// Handles both HTTPS and SSH URL formats:
/// - `https://github.com/user/repo.git` → `repo`
/// - `git@github.com:user/repo.git` → `repo`
/// - `https://dev.azure.com/org/project/_git/repo` → `repo`
fn parse_repo_name_from_url(url: &str) -> Option<String> {
    let url = url.trim();

    // Strip query/fragment so `.git?foo=bar` still matches the `.git` suffix below.
    let url = url.split_once(['?', '#']).map_or(url, |(before, _)| before);
    let url = url.strip_suffix(".git").unwrap_or(url);

    // Filter empty segments so a trailing `/` doesn't yield `""`; the
    // caller's fallback (directory name) expects `None`, not an empty string.
    url.rsplit(['/', ':'])
        .find(|s| !s.is_empty())
        .map(str::to_string)
}

/// Get the project root path
///
/// Finds the root path of the Rust project by:
/// 1. Checking Cargo environment variables (when building with cargo)
/// 2. Searching upward for a Cargo.toml file (for local development)
/// 3. Falling back to the current directory if all else fails
///
/// This approach ensures the function works correctly in both local development
/// and when the crate is used as a dependency in other projects.
///
/// # Returns
/// The path to the project root directory
pub fn get_project_path() -> PathBuf {
    if let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") {
        return PathBuf::from(manifest_dir);
    }

    let mut path = match env::current_dir() {
        Ok(p) => p,
        Err(_) => PathBuf::from("."),
    };

    while !path.join("Cargo.toml").exists() {
        if !path.pop() {
            return match env::current_dir() {
                Ok(p) => p,
                Err(_) => PathBuf::from("."),
            };
        }
    }

    path
}

/// Get the content of Cargo.toml file as a TOML value
///
/// Reads and parses the Cargo.toml file at the project root.
///
/// # Returns
/// A parsed TOML Value representing the Cargo.toml content
///
/// # Errors
/// Returns an error if the file cannot be read or parsed
#[must_use = "the parsed Cargo.toml is the sole output of this function; discarding it wastes the I/O and parse work"]
pub fn get_cargo_toml_content() -> ModuleInfoResult<Value> {
    let project_path = get_project_path();
    let cargo_path = project_path.join("Cargo.toml");

    let mut file = File::open(cargo_path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;

    let cargo_toml: Value = toml::from_str(&content)?;
    Ok(cargo_toml)
}

#[cfg(test)]
mod tests {
    use super::parse_repo_name_from_url;

    #[test]
    fn parses_common_remote_shapes() {
        // (input, expected)
        let cases = [
            ("https://github.com/user/repo.git", Some("repo".to_string())),
            ("https://github.com/user/repo", Some("repo".to_string())),
            ("git@github.com:user/repo.git", Some("repo".to_string())),
            (
                "https://dev.azure.com/org/project/_git/repo",
                Some("repo".to_string()),
            ),
            // Trailing slash: empty-segment filter falls back to the last
            // non-empty path component.
            ("https://github.com/user/repo/", Some("repo".to_string())),
            // Pure whitespace / empty input: no segment survives filtering.
            ("", None),
            ("   ", None),
            ("/", None),
        ];
        for (input, expected) in cases {
            assert_eq!(
                parse_repo_name_from_url(input),
                expected,
                "unexpected result for input {input:?}"
            );
        }
    }
}
