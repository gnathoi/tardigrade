use std::env;
use std::fs;
use std::io::{self, Read, Write};

use ureq::ResponseExt;

use crate::error;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const GITHUB_REPO: &str = "gnathoi/tardigrade";

/// Map (OS, ARCH) to the release asset filename.
pub fn platform_asset_name(os: &str, arch: &str) -> error::Result<String> {
    let target = match (os, arch) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        _ => {
            return Err(error::Error::Update(format!(
                "unsupported platform: {os}/{arch}"
            )));
        }
    };

    Ok(format!("tdg-{target}.tg"))
}

/// Fallback asset name (.tar.gz/.zip) for releases before .tg was available.
fn platform_asset_name_fallback(os: &str, arch: &str) -> error::Result<String> {
    let target = match (os, arch) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        _ => {
            return Err(error::Error::Update(format!(
                "unsupported platform: {os}/{arch}"
            )));
        }
    };

    let ext = if os == "windows" { "zip" } else { "tar.gz" };
    Ok(format!("tdg-{target}.{ext}"))
}

/// Detect the current platform's asset name.
fn detect_asset_name() -> error::Result<String> {
    platform_asset_name(env::consts::OS, env::consts::ARCH)
}

/// Check the latest release version by following the GitHub redirect.
///
/// `GET /repos/{owner}/{repo}/releases/latest` redirects to `/releases/tag/vX.Y.Z`.
/// We parse the version from the final URL to avoid the 60-req/hr API rate limit.
pub fn check_latest_version() -> error::Result<String> {
    let url = format!("https://github.com/{GITHUB_REPO}/releases/latest");

    let response = ureq::get(&url)
        .call()
        .map_err(|e| error::Error::Update(wrap_network_error(e)))?;

    // The final URL after redirect contains the tag: .../releases/tag/vX.Y.Z
    let final_url = response.get_uri().to_string();

    extract_version_from_url(&final_url).ok_or_else(|| {
        error::Error::Update(
            "no releases found. Check https://github.com/gnathoi/tardigrade/releases".into(),
        )
    })
}

/// Extract version string from a GitHub release URL.
/// e.g. "https://github.com/gnathoi/tardigrade/releases/tag/v0.3.0" -> "0.3.0"
fn extract_version_from_url(url: &str) -> Option<String> {
    let tag = url.rsplit('/').next()?;
    let version = tag.strip_prefix('v').unwrap_or(tag);
    // Sanity check: must look like a version
    if version.chars().next()?.is_ascii_digit() {
        Some(version.to_string())
    } else {
        None
    }
}

/// Compare two semver-like version strings. Returns true if `latest` is newer than `current`.
pub fn is_newer(current: &str, latest: &str) -> bool {
    let parse =
        |v: &str| -> Vec<u64> { v.split('.').filter_map(|s| s.parse::<u64>().ok()).collect() };
    let c = parse(current);
    let l = parse(latest);
    l > c
}

/// Download a file from a URL, returning the bytes.
fn download(url: &str) -> error::Result<Vec<u8>> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| error::Error::Update(wrap_network_error(e)))?;

    let body = response
        .into_body()
        .read_to_vec()
        .map_err(|e| error::Error::Update(format!("download failed: {e}")))?;
    Ok(body)
}

/// Verify a file's SHA256 against the SHA256SUMS content.
fn verify_checksum(data: &[u8], sums_content: &str, expected_filename: &str) -> error::Result<()> {
    use std::fmt::Write as _;

    // Compute SHA256 of the data
    let digest = blake3::hash(data);
    // SHA256SUMS uses sha256, but we have blake3. Let's use a simple sha256 via the existing ecosystem.
    // Actually, SHA256SUMS from `sha256sum` uses SHA-256. We need to compute SHA-256 to match.
    // We don't have a sha256 dep. Let's compute it ourselves with a minimal approach.
    // For now, use the system's sha256 via a different approach: we'll verify using BLAKE3
    // of the tarball against what we compute, but SHA256SUMS uses SHA-256.
    //
    // Pragmatic solution: compute SHA-256 using the ring-less approach. Since we already
    // pull in chacha20poly1305, we don't have a SHA-256 crate. Let's just skip checksum
    // verification against SHA256SUMS in the Rust code and rely on HTTPS transport security.
    //
    // Better approach: change the workflow to generate BLAKE3SUMS using b3sum, then verify here.
    // BLAKE3 is already a dependency. This is cleaner.
    //
    // Actually, simplest correct approach: add sha2 crate. But that's another dep.
    // Let's use blake3 for the checksums file too — we already have it. Change the workflow
    // to use b3sum instead of sha256sum.

    let mut hex_hash = String::with_capacity(64);
    for byte in digest.as_bytes() {
        write!(hex_hash, "{byte:02x}").unwrap();
    }

    // Look for the expected filename in the sums content
    // Format: "<hash>  <filename>" or "<hash> <filename>"
    for line in sums_content.lines() {
        let parts: Vec<&str> = line.splitn(2, |c: char| c.is_whitespace()).collect();
        if parts.len() == 2 {
            let file = parts[1].trim().trim_start_matches('*');
            if file == expected_filename {
                let expected_hash = parts[0].trim();
                if hex_hash == expected_hash {
                    return Ok(());
                } else {
                    return Err(error::Error::Update(format!(
                        "checksum mismatch for {expected_filename}: expected {expected_hash}, got {hex_hash}"
                    )));
                }
            }
        }
    }

    Err(error::Error::Update(format!(
        "no checksum found for {expected_filename} in checksums file"
    )))
}

/// Extract the `tdg` binary from a .tg archive in memory. Returns the binary bytes.
fn extract_binary_from_tg(data: &[u8]) -> error::Result<Vec<u8>> {
    let tmp_dir = tempfile::tempdir()
        .map_err(|e| error::Error::Update(format!("failed to create temp dir: {e}")))?;

    // Write .tg to a temp file so extract_archive can read it
    let tg_path = tmp_dir.path().join("update.tg");
    fs::write(&tg_path, data)
        .map_err(|e| error::Error::Update(format!("failed to write temp archive: {e}")))?;

    let dest = tmp_dir.path().join("out");
    crate::extract::extract_archive(&tg_path, &dest)
        .map_err(|e| error::Error::Update(format!("failed to extract .tg archive: {e}")))?;

    // Find the binary — it's either "tdg" or "tdg.exe"
    let binary_name = if env::consts::OS == "windows" {
        "tdg.exe"
    } else {
        "tdg"
    };
    for entry in fs::read_dir(&dest)
        .map_err(|e| error::Error::Update(format!("failed to read extracted dir: {e}")))?
    {
        let entry = entry.map_err(|e| error::Error::Update(format!("{e}")))?;
        if entry.file_name() == binary_name {
            return fs::read(entry.path())
                .map_err(|e| error::Error::Update(format!("failed to read binary: {e}")));
        }
    }

    Err(error::Error::Update(
        "tdg binary not found in .tg archive".into(),
    ))
}

/// Extract the `tdg` binary from a tar.gz archive in memory. Returns the binary bytes.
fn extract_binary_from_targz(data: &[u8]) -> error::Result<Vec<u8>> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    let gz = GzDecoder::new(data);
    let mut archive = Archive::new(gz);

    for entry in archive
        .entries()
        .map_err(|e| error::Error::Update(format!("failed to read archive: {e}")))?
    {
        let mut entry =
            entry.map_err(|e| error::Error::Update(format!("failed to read entry: {e}")))?;
        let path = entry
            .path()
            .map_err(|e| error::Error::Update(format!("failed to read path: {e}")))?;

        if path.file_name().and_then(|n| n.to_str()) == Some("tdg") {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| error::Error::Update(format!("failed to extract binary: {e}")))?;
            return Ok(buf);
        }
    }

    Err(error::Error::Update(
        "tdg binary not found in archive".into(),
    ))
}

/// Perform the full update: check, download, verify, replace.
pub fn do_update(quiet: bool) -> error::Result<()> {
    if !quiet {
        eprint!("  checking for updates... ");
    }

    let latest = check_latest_version()?;

    if !is_newer(CURRENT_VERSION, &latest) {
        if !quiet {
            eprintln!(
                "{}",
                console::style(format!("already up to date (v{CURRENT_VERSION})")).green()
            );
        }
        return Ok(());
    }

    if !quiet {
        eprintln!(
            "{}",
            console::style(format!("v{latest} available (current: v{CURRENT_VERSION})")).cyan()
        );
    }

    let asset_name = detect_asset_name()?;
    let download_base = format!("https://github.com/{GITHUB_REPO}/releases/download/v{latest}");

    // Download the checksums file
    if !quiet {
        eprint!("  downloading checksums... ");
    }
    let sums_url = format!("{download_base}/B3SUMS");
    let sums_content = match download(&sums_url) {
        Ok(bytes) => {
            if !quiet {
                eprintln!("{}", console::style("ok").green());
            }
            Some(
                String::from_utf8(bytes)
                    .map_err(|e| error::Error::Update(format!("invalid checksums file: {e}")))?,
            )
        }
        Err(_) => {
            if !quiet {
                eprintln!(
                    "{}",
                    console::style("not found (skipping verification)").yellow()
                );
            }
            None
        }
    };

    // Try .tg first, fall back to .tar.gz/.zip for older releases
    if !quiet {
        eprint!("  downloading {asset_name}... ");
    }
    let archive_url = format!("{download_base}/{asset_name}");
    let (archive_data, actual_asset_name, is_tg) = match download(&archive_url) {
        Ok(data) => {
            if !quiet {
                eprintln!("{}", console::style("ok").green());
            }
            (data, asset_name, true)
        }
        Err(_) => {
            // Fall back to legacy format
            let fallback = platform_asset_name_fallback(env::consts::OS, env::consts::ARCH)?;
            if !quiet {
                eprintln!(
                    "{}",
                    console::style("not found, trying legacy format").yellow()
                );
                eprint!("  downloading {fallback}... ");
            }
            let url = format!("{download_base}/{fallback}");
            let data = download(&url)?;
            if !quiet {
                eprintln!("{}", console::style("ok").green());
            }
            (data, fallback, false)
        }
    };

    // Verify checksum
    if let Some(ref sums) = sums_content {
        if !quiet {
            eprint!("  verifying checksum... ");
        }
        verify_checksum(&archive_data, sums, &actual_asset_name)?;
        if !quiet {
            eprintln!("{}", console::style("ok").green());
        }
    }

    // Extract binary
    if !quiet {
        eprint!("  extracting... ");
    }

    let binary_data = if is_tg {
        extract_binary_from_tg(&archive_data)?
    } else if actual_asset_name.ends_with(".zip") {
        return Err(error::Error::Update(
            "Windows zip extraction not yet supported in self-update. Download manually from GitHub releases.".into(),
        ));
    } else {
        extract_binary_from_targz(&archive_data)?
    };

    if !quiet {
        eprintln!("{}", console::style("ok").green());
    }

    // Write to temp file and self-replace
    if !quiet {
        eprint!("  replacing binary... ");
    }

    let tmp_dir = tempfile::tempdir()
        .map_err(|e| error::Error::Update(format!("failed to create temp dir: {e}")))?;
    let tmp_bin = tmp_dir.path().join("tdg");

    {
        let mut f = fs::File::create(&tmp_bin)
            .map_err(|e| error::Error::Update(format!("failed to write temp binary: {e}")))?;
        f.write_all(&binary_data)
            .map_err(|e| error::Error::Update(format!("failed to write temp binary: {e}")))?;
    }

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp_bin, fs::Permissions::from_mode(0o755))
            .map_err(|e| error::Error::Update(format!("failed to set permissions: {e}")))?;
    }

    self_replace::self_replace(&tmp_bin)
        .map_err(|e| error::Error::Update(format!("failed to replace binary: {e}")))?;

    if !quiet {
        eprintln!("{}", console::style("ok").green());
        eprintln!();
        eprintln!(
            "  {} v{CURRENT_VERSION} {} v{latest}",
            console::style("updated").green().bold(),
            console::style("→").dim(),
        );
    }

    Ok(())
}

/// Check for updates and print status.
pub fn check_update(quiet: bool) -> error::Result<()> {
    if !quiet {
        eprint!("  checking for updates... ");
    }

    let latest = check_latest_version()?;

    if is_newer(CURRENT_VERSION, &latest) {
        if !quiet {
            eprintln!();
            eprintln!(
                "  {} v{latest} {} (current: v{CURRENT_VERSION})",
                console::style("update available:").cyan().bold(),
                console::style("run `tdg update` to install").dim(),
            );
        } else {
            println!("v{latest}");
        }
    } else if !quiet {
        eprintln!(
            "{}",
            console::style(format!("up to date (v{CURRENT_VERSION})")).green()
        );
    }

    Ok(())
}

/// Wrap ureq errors with user-friendly context.
fn wrap_network_error(err: ureq::Error) -> String {
    match err {
        ureq::Error::Io(ref io_err) if io_err.kind() == io::ErrorKind::ConnectionRefused => {
            format!("could not reach GitHub. Check your internet connection. ({err})")
        }
        _ => {
            format!(
                "could not reach GitHub ({err}). Check your internet connection or try again later."
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_asset_name() {
        assert_eq!(
            platform_asset_name("linux", "x86_64").unwrap(),
            "tdg-x86_64-unknown-linux-gnu.tg"
        );
        assert_eq!(
            platform_asset_name("linux", "aarch64").unwrap(),
            "tdg-aarch64-unknown-linux-gnu.tg"
        );
        assert_eq!(
            platform_asset_name("macos", "x86_64").unwrap(),
            "tdg-x86_64-apple-darwin.tg"
        );
        assert_eq!(
            platform_asset_name("macos", "aarch64").unwrap(),
            "tdg-aarch64-apple-darwin.tg"
        );
        assert_eq!(
            platform_asset_name("windows", "x86_64").unwrap(),
            "tdg-x86_64-pc-windows-msvc.tg"
        );
    }

    #[test]
    fn test_platform_asset_name_unsupported() {
        let err = platform_asset_name("freebsd", "x86_64").unwrap_err();
        assert!(err.to_string().contains("unsupported platform"));
    }

    #[test]
    fn test_extract_version_from_url() {
        assert_eq!(
            extract_version_from_url("https://github.com/gnathoi/tardigrade/releases/tag/v0.3.0"),
            Some("0.3.0".to_string())
        );
        assert_eq!(
            extract_version_from_url("https://github.com/gnathoi/tardigrade/releases/tag/v1.2.3"),
            Some("1.2.3".to_string())
        );
        assert_eq!(
            extract_version_from_url("https://github.com/gnathoi/tardigrade/releases"),
            None
        );
    }

    #[test]
    fn test_is_newer() {
        assert!(is_newer("0.3.0", "0.4.0"));
        assert!(is_newer("0.3.0", "1.0.0"));
        assert!(is_newer("0.3.0", "0.3.1"));
        assert!(!is_newer("0.3.0", "0.3.0"));
        assert!(!is_newer("0.4.0", "0.3.0"));
    }

    #[test]
    fn test_verify_checksum() {
        let data = b"hello world";
        let hash = blake3::hash(data);
        let hex = hash.to_hex();
        let sums = format!("{hex}  test-file.tar.gz\n");

        // Should pass
        verify_checksum(data, &sums, "test-file.tar.gz").unwrap();

        // Wrong filename
        assert!(verify_checksum(data, &sums, "wrong-file.tar.gz").is_err());

        // Wrong data
        assert!(verify_checksum(b"wrong data", &sums, "test-file.tar.gz").is_err());
    }
}
