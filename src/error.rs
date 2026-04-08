use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {source}")]
    Io {
        #[from]
        source: std::io::Error,
    },

    #[error("I/O error at {path}: {source}")]
    IoPath {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("invalid archive: {0}")]
    InvalidArchive(String),

    #[error("unsupported archive version: {0}")]
    UnsupportedVersion(u16),

    #[error("unknown compression codec: {0}")]
    UnknownCodec(u8),

    #[error("compression error: {0}")]
    Compression(String),

    #[error("decompression error: {0}")]
    Decompression(String),

    #[error("checksum mismatch for block at offset {offset}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        offset: u64,
        expected: String,
        actual: String,
    },

    #[error("header CRC mismatch at offset {offset}")]
    HeaderCrcMismatch { offset: u64 },

    #[error("index deserialization error: {0}")]
    IndexDeserialize(String),

    #[error("path traversal rejected: {0}")]
    PathTraversal(String),

    #[error("symlink target escapes destination: {path} -> {target}")]
    SymlinkEscape { path: PathBuf, target: PathBuf },

    #[error("archive is encrypted — use --encrypt to provide passphrase or --identity <keyfile>")]
    EncryptedArchive,

    #[error("volume: {0}")]
    Volume(String),

    #[error("ecc: {0}")]
    Ecc(String),

    #[error("no snapshots found in archive")]
    NoSnapshots,

    #[error("update: {0}")]
    Update(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn io_path(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::IoPath {
            path: path.into(),
            source,
        }
    }
}
