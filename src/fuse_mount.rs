/// FUSE read-only mounting of .tg archives.
///
/// `tdg mount archive.tg /mnt/point`
/// `tdg mount archive.tg@3 /mnt/point` (temporal: generation 3)
///
/// Linux: full support via fuser crate (libfuse3)
/// macOS: best-effort via macFUSE
/// Windows/BSDs: not supported, use `tdg serve` (HTTP browse)

// Stubbed for v0.1. Requires platform-specific FUSE bindings.

pub(crate) fn _stub() {}
