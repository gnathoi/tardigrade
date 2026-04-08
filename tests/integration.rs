// Integration tests: exercise the CLI commands end-to-end via the binary.
//
// These tests build and run the actual `tdg` binary, verifying that all
// new commands work correctly as a user would experience them.

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

/// Get the path to the built tdg binary
fn tdg() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tdg"));
    cmd.env("NO_COLOR", "1"); // Disable color output for easier assertion
    cmd
}

/// Create a test directory with some files
fn create_test_dir(dir: &Path) {
    fs::create_dir_all(dir.join("subdir")).unwrap();
    fs::write(dir.join("hello.txt"), "Hello, tardigrade!").unwrap();
    fs::write(dir.join("world.txt"), "World data here.").unwrap();
    fs::write(dir.join("subdir/nested.txt"), "Nested file content.").unwrap();
    // A larger file for dedup testing
    fs::write(dir.join("big.txt"), "x".repeat(100_000)).unwrap();
    fs::write(dir.join("big_copy.txt"), "x".repeat(100_000)).unwrap();
}

// ─── Basic commands ────────────────────────────────────────────────────────

#[test]
fn cli_version() {
    let output = tdg().arg("--version").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("0.2.0"), "version: {stdout}");
}

#[test]
fn cli_help() {
    let output = tdg().arg("--help").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("create"));
    assert!(stdout.contains("extract"));
    assert!(stdout.contains("merge"));
    assert!(stdout.contains("split"));
    assert!(stdout.contains("join"));
    assert!(stdout.contains("convert"));
    assert!(stdout.contains("log"));
}

// ─── Create + Extract round trip ───────────────────────────────────────────

#[test]
fn cli_create_extract_round_trip() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    create_test_dir(&src);

    let archive = tmp.path().join("test.tg");
    let dest = tmp.path().join("extracted");

    // Create
    let output = tdg()
        .args(["create", archive.to_str().unwrap(), src.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(archive.exists());

    // Extract
    let output = tdg()
        .args([
            "extract",
            archive.to_str().unwrap(),
            "-o",
            dest.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "extract failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(dest.join("hello.txt")).unwrap(),
        "Hello, tardigrade!"
    );
    assert_eq!(
        fs::read_to_string(dest.join("subdir/nested.txt")).unwrap(),
        "Nested file content."
    );
}

// ─── Info ──────────────────────────────────────────────────────────────────

#[test]
fn cli_info() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    create_test_dir(&src);

    let archive = tmp.path().join("info.tg");
    tdg()
        .args([
            "create",
            archive.to_str().unwrap(),
            src.to_str().unwrap(),
            "-q",
        ])
        .output()
        .unwrap();

    let output = tdg()
        .args(["info", archive.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("TRDG v1"));
    assert!(stdout.contains("Files:"));
}

// ─── Verify ────────────────────────────────────────────────────────────────

#[test]
fn cli_verify() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    create_test_dir(&src);

    let archive = tmp.path().join("verify.tg");
    tdg()
        .args([
            "create",
            archive.to_str().unwrap(),
            src.to_str().unwrap(),
            "-q",
        ])
        .output()
        .unwrap();

    let output = tdg()
        .args(["verify", archive.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("verified"));
    assert!(stdout.contains("0 corrupted"));
}

// ─── List ──────────────────────────────────────────────────────────────────

#[test]
fn cli_list() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    create_test_dir(&src);

    let archive = tmp.path().join("list.tg");
    tdg()
        .args([
            "create",
            archive.to_str().unwrap(),
            src.to_str().unwrap(),
            "-q",
        ])
        .output()
        .unwrap();

    // Short list
    let output = tdg()
        .args(["list", archive.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("hello.txt"));

    // Long list
    let output = tdg()
        .args(["list", "-l", archive.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("entries"));
}

// ─── Temporal: append + log + extract generation ───────────────────────────

#[test]
fn cli_temporal_workflow() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("v1.txt"), "version 1 content").unwrap();

    let archive = tmp.path().join("temporal.tg");

    // Create initial archive
    let output = tdg()
        .args([
            "create",
            archive.to_str().unwrap(),
            src.to_str().unwrap(),
            "-q",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "initial create failed");

    // Modify source and append
    fs::write(src.join("v1.txt"), "version 2 content").unwrap();
    fs::write(src.join("v2.txt"), "new in gen 1").unwrap();

    let output = tdg()
        .args([
            "create",
            "--append",
            archive.to_str().unwrap(),
            src.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "append failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("appended") || stdout.contains("generation"),
        "expected 'appended' in: {stdout}"
    );

    // Log
    let output = tdg()
        .args(["log", archive.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("2 generations") || stdout.contains("@0"));
    assert!(stdout.contains("@1"));

    // Extract generation 0 (original)
    let dest0 = tmp.path().join("gen0");
    let output = tdg()
        .args([
            "extract",
            "--generation",
            "0",
            archive.to_str().unwrap(),
            "-o",
            dest0.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "extract gen 0 failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(dest0.join("v1.txt")).unwrap(),
        "version 1 content"
    );
    assert!(
        !dest0.join("v2.txt").exists(),
        "v2.txt should not exist in gen 0"
    );

    // Extract generation 1 (appended)
    let dest1 = tmp.path().join("gen1");
    let output = tdg()
        .args([
            "extract",
            "--generation",
            "1",
            archive.to_str().unwrap(),
            "-o",
            dest1.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "extract gen 1 failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(dest1.join("v1.txt")).unwrap(),
        "version 2 content"
    );
    assert_eq!(
        fs::read_to_string(dest1.join("v2.txt")).unwrap(),
        "new in gen 1"
    );
}

// ─── Incremental: create + extract ─────────────────────────────────────────

#[test]
fn cli_incremental_workflow() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    let shared = "shared content".repeat(500);
    fs::write(src.join("shared.txt"), &shared).unwrap();
    fs::write(src.join("old.txt"), "old content").unwrap();

    let base = tmp.path().join("base.tg");
    tdg()
        .args([
            "create",
            base.to_str().unwrap(),
            src.to_str().unwrap(),
            "-q",
        ])
        .output()
        .unwrap();

    // Modify source
    fs::write(src.join("new.txt"), "brand new file").unwrap();

    let diff = tmp.path().join("diff.tg");
    let output = tdg()
        .args([
            "create",
            "--incremental",
            base.to_str().unwrap(),
            diff.to_str().unwrap(),
            src.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "incremental create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("incremental") || stdout.contains("reused"),
        "expected 'incremental' in: {stdout}"
    );

    // Incremental archive should be smaller than base
    let base_size = fs::metadata(&base).unwrap().len();
    let diff_size = fs::metadata(&diff).unwrap().len();
    assert!(
        diff_size < base_size,
        "incremental ({diff_size}) should be smaller than base ({base_size})"
    );

    // Extract with base
    let dest = tmp.path().join("extracted");
    let output = tdg()
        .args([
            "extract",
            "--base",
            base.to_str().unwrap(),
            diff.to_str().unwrap(),
            "-o",
            dest.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "incremental extract failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(dest.join("shared.txt")).unwrap(), shared);
    assert_eq!(
        fs::read_to_string(dest.join("new.txt")).unwrap(),
        "brand new file"
    );
}

// ─── Merge ─────────────────────────────────────────────────────────────────

#[test]
fn cli_merge_workflow() {
    let tmp = TempDir::new().unwrap();

    let src_a = tmp.path().join("a");
    fs::create_dir_all(&src_a).unwrap();
    fs::write(src_a.join("only_a.txt"), "from archive A").unwrap();
    fs::write(src_a.join("shared.txt"), "shared content".repeat(500)).unwrap();

    let src_b = tmp.path().join("b");
    fs::create_dir_all(&src_b).unwrap();
    fs::write(src_b.join("only_b.txt"), "from archive B").unwrap();
    fs::write(src_b.join("shared.txt"), "shared content".repeat(500)).unwrap();

    let a_tg = tmp.path().join("a.tg");
    let b_tg = tmp.path().join("b.tg");
    tdg()
        .args([
            "create",
            a_tg.to_str().unwrap(),
            src_a.to_str().unwrap(),
            "-q",
        ])
        .output()
        .unwrap();
    tdg()
        .args([
            "create",
            b_tg.to_str().unwrap(),
            src_b.to_str().unwrap(),
            "-q",
        ])
        .output()
        .unwrap();

    let merged = tmp.path().join("merged.tg");
    let output = tdg()
        .args([
            "merge",
            a_tg.to_str().unwrap(),
            b_tg.to_str().unwrap(),
            "-o",
            merged.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "merge failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("merged"));

    // Verify merged archive contains both
    let dest = tmp.path().join("extracted");
    tdg()
        .args([
            "extract",
            merged.to_str().unwrap(),
            "-o",
            dest.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        dest.join("only_a.txt").exists(),
        "only_a.txt missing from merge"
    );
    assert!(
        dest.join("only_b.txt").exists(),
        "only_b.txt missing from merge"
    );
    assert!(
        dest.join("shared.txt").exists(),
        "shared.txt missing from merge"
    );

    // Merged archive should dedup shared content
    let a_size = fs::metadata(&a_tg).unwrap().len();
    let b_size = fs::metadata(&b_tg).unwrap().len();
    let merged_size = fs::metadata(&merged).unwrap().len();
    assert!(
        merged_size < a_size + b_size,
        "merged ({merged_size}) should be smaller than sum ({}) due to dedup",
        a_size + b_size
    );
}

// ─── Split + Join ──────────────────────────────────────────────────────────

#[test]
fn cli_split_join_workflow() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();

    // Create enough data to split
    for i in 0..50 {
        fs::write(
            src.join(format!("file_{i}.txt")),
            format!("content {i}").repeat(2000),
        )
        .unwrap();
    }

    let archive = tmp.path().join("big.tg");
    tdg()
        .args([
            "create",
            archive.to_str().unwrap(),
            src.to_str().unwrap(),
            "-q",
        ])
        .output()
        .unwrap();

    let archive_size = fs::metadata(&archive).unwrap().len();

    // Split
    let vol_size = format!("{}", std::cmp::max(archive_size / 3, 2048));
    let output = tdg()
        .args(["split", archive.to_str().unwrap(), "--size", &vol_size])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "split failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("split"));

    // Find the volumes
    let mut volumes: Vec<_> = fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.starts_with("big.") && name.ends_with(".tg") && name.contains("00")
        })
        .map(|e| e.path())
        .collect();
    volumes.sort();
    assert!(
        volumes.len() >= 2,
        "expected at least 2 volumes, got {}",
        volumes.len()
    );

    // Join
    let joined = tmp.path().join("joined.tg");
    let mut args: Vec<String> = vec!["join".into()];
    for v in &volumes {
        args.push(v.to_str().unwrap().into());
    }
    args.push("-o".into());
    args.push(joined.to_str().unwrap().into());

    let output = tdg()
        .args(args.iter().map(|s| s.as_str()).collect::<Vec<_>>())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "join failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify joined matches original
    let original = fs::read(&archive).unwrap();
    let rejoined = fs::read(&joined).unwrap();
    assert_eq!(original, rejoined, "joined archive should match original");

    // Extract from joined and verify
    let dest = tmp.path().join("extracted");
    let output = tdg()
        .args([
            "extract",
            joined.to_str().unwrap(),
            "-o",
            dest.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(dest.join("file_0.txt")).unwrap(),
        "content 0".repeat(2000)
    );
}

// ─── Convert (tar -> tg) ──────────────────────────────────────────────────

#[test]
fn cli_convert_tar() {
    let tmp = TempDir::new().unwrap();
    let tar_path = tmp.path().join("test.tar");

    // Create a real tar file
    {
        let file = fs::File::create(&tar_path).unwrap();
        let mut builder = tar::Builder::new(file);

        let data = b"hello from tar integration test";
        let mut header = tar::Header::new_gnu();
        header.set_path("hello.txt").unwrap();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, &data[..]).unwrap();

        let data2 = b"second file content";
        let mut header2 = tar::Header::new_gnu();
        header2.set_path("second.txt").unwrap();
        header2.set_size(data2.len() as u64);
        header2.set_mode(0o644);
        header2.set_cksum();
        builder.append(&header2, &data2[..]).unwrap();

        builder.finish().unwrap();
    }

    // Convert to .tg
    let tg_path = tmp.path().join("converted.tg");
    let output = tdg()
        .args([
            "convert",
            tar_path.to_str().unwrap(),
            tg_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "convert failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("converted"));

    // Extract the .tg and verify
    let dest = tmp.path().join("extracted");
    let output = tdg()
        .args([
            "extract",
            tg_path.to_str().unwrap(),
            "-o",
            dest.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(dest.join("hello.txt").exists());
    assert!(dest.join("second.txt").exists());

    // Verify the converted archive with tdg verify
    let output = tdg()
        .args(["verify", tg_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
}

// ─── Extract auto-detects tar format ───────────────────────────────────────

#[test]
fn cli_extract_tar_auto_detect() {
    let tmp = TempDir::new().unwrap();
    let tar_path = tmp.path().join("auto.tar");

    {
        let file = fs::File::create(&tar_path).unwrap();
        let mut builder = tar::Builder::new(file);

        let data = b"auto-detected tar content";
        let mut header = tar::Header::new_gnu();
        header.set_path("auto.txt").unwrap();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, &data[..]).unwrap();
        builder.finish().unwrap();
    }

    let dest = tmp.path().join("extracted");
    let output = tdg()
        .args([
            "extract",
            tar_path.to_str().unwrap(),
            "-o",
            dest.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "auto-detect extract failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(dest.join("auto.txt")).unwrap(),
        "auto-detected tar content"
    );
}

// ─── Extract tar.gz auto-detect ────────────────────────────────────────────

#[test]
fn cli_extract_tar_gz_auto_detect() {
    use flate2::write::GzEncoder;

    let tmp = TempDir::new().unwrap();
    let targz_path = tmp.path().join("test.tar.gz");

    {
        let file = fs::File::create(&targz_path).unwrap();
        let gz = GzEncoder::new(file, flate2::Compression::fast());
        let mut builder = tar::Builder::new(gz);

        let data = b"gzip compressed tar";
        let mut header = tar::Header::new_gnu();
        header.set_path("gz.txt").unwrap();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, &data[..]).unwrap();
        builder.into_inner().unwrap().finish().unwrap();
    }

    let dest = tmp.path().join("extracted");
    let output = tdg()
        .args([
            "extract",
            targz_path.to_str().unwrap(),
            "-o",
            dest.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(dest.join("gz.txt")).unwrap(),
        "gzip compressed tar"
    );
}

// ─── ECC flag accepted ────────────────────────────────────────────────────

#[test]
fn cli_ecc_flag() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("data.txt"), "ecc test data").unwrap();

    let archive = tmp.path().join("ecc.tg");
    let output = tdg()
        .args([
            "create",
            "--ecc",
            "low",
            archive.to_str().unwrap(),
            src.to_str().unwrap(),
            "-q",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "ecc create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(archive.exists());

    // Should still be extractable
    let dest = tmp.path().join("extracted");
    let output = tdg()
        .args([
            "extract",
            archive.to_str().unwrap(),
            "-o",
            dest.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(dest.join("data.txt")).unwrap(),
        "ecc test data"
    );
}

// ─── Quiet mode ────────────────────────────────────────────────────────────

#[test]
fn cli_quiet_mode() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("q.txt"), "quiet").unwrap();

    let archive = tmp.path().join("quiet.tg");
    let output = tdg()
        .args([
            "-q",
            "create",
            archive.to_str().unwrap(),
            src.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.is_empty() || stdout.trim().is_empty(),
        "quiet mode should produce no output, got: {stdout}"
    );
}

// ─── Compression options ───────────────────────────────────────────────────

#[test]
fn cli_lz4_compression() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("lz4.txt"), "lz4 test data".repeat(100)).unwrap();

    let archive = tmp.path().join("lz4.tg");
    let output = tdg()
        .args([
            "create",
            "--compress",
            "lz4",
            archive.to_str().unwrap(),
            src.to_str().unwrap(),
            "-q",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    let dest = tmp.path().join("extracted");
    let output = tdg()
        .args([
            "extract",
            archive.to_str().unwrap(),
            "-o",
            dest.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(dest.join("lz4.txt")).unwrap(),
        "lz4 test data".repeat(100)
    );
}

// ─── Error cases ───────────────────────────────────────────────────────────

#[test]
fn cli_extract_nonexistent_file() {
    let output = tdg()
        .args(["extract", "/tmp/does_not_exist_tdg_test.tg"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error:"),
        "expected error message, got: {stderr}"
    );
}

#[test]
fn cli_merge_nonexistent() {
    let output = tdg()
        .args([
            "merge",
            "/tmp/nope1.tg",
            "/tmp/nope2.tg",
            "-o",
            "/tmp/out.tg",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn cli_convert_non_tar() {
    let tmp = TempDir::new().unwrap();
    let not_tar = tmp.path().join("not_tar.bin");
    fs::write(&not_tar, "this is definitely not a tar file").unwrap();

    let output = tdg()
        .args([
            "convert",
            not_tar.to_str().unwrap(),
            tmp.path().join("out.tg").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error:"));
}
