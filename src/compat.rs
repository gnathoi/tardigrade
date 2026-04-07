/// Read-only tar.zst extraction for migration.
///
/// `tdg extract legacy.tar.zst` detects tar magic and decompresses
/// the zstd wrapper, extracting via the tar crate.
///
/// This is a migration path: users can adopt the tdg CLI without
/// immediately adopting the .tg format.

// Stubbed for v0.1. Will use the `tar` crate for tar format reading.

pub(crate) fn _stub() {}
