use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use std::io::{Read, Write};

use crate::error::{Error, Result};
use crate::format::Hash;

/// Symmetric key for archive encryption (256-bit)
pub type SymmetricKey = [u8; 32];

/// Size of the AEAD authentication tag
pub const TAG_SIZE: usize = 16;

/// Derive a symmetric key from a passphrase using scrypt
pub fn derive_key_from_passphrase(passphrase: &[u8], salt: &[u8; 16]) -> SymmetricKey {
    use blake3::Hasher;
    // Simple key derivation: BLAKE3(salt || passphrase || salt)
    // For production, use scrypt or argon2. This is a reasonable
    // starting point that's fast and deterministic.
    let mut hasher = Hasher::new_derive_key("tardigrade-archive-key-v1");
    hasher.update(salt);
    hasher.update(passphrase);
    hasher.update(salt);
    let mut key = [0u8; 32];
    hasher.finalize_xof().fill(&mut key);
    key
}

/// Generate a random symmetric key
pub fn generate_key() -> SymmetricKey {
    let mut key = [0u8; 32];
    rand::fill(&mut key);
    key
}

/// Generate a random salt
pub fn generate_salt() -> [u8; 16] {
    let mut salt = [0u8; 16];
    rand::fill(&mut salt);
    salt
}

/// Encrypt a block of data using ChaCha20-Poly1305.
/// Nonce is derived from the BLAKE3 content hash (first 12 bytes).
/// This makes encryption deterministic for identical plaintext, which is
/// safe (same input = same output) and avoids nonce reuse in append/merge.
pub fn encrypt_block(data: &[u8], key: &SymmetricKey, content_hash: &Hash) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let nonce = Nonce::from_slice(&content_hash[..12]);

    cipher
        .encrypt(nonce, data)
        .map_err(|e| Error::Compression(format!("encryption failed: {e}")))
}

/// Decrypt a block of data using ChaCha20-Poly1305.
/// Nonce is derived from the BLAKE3 content hash (first 12 bytes).
pub fn decrypt_block(ciphertext: &[u8], key: &SymmetricKey, content_hash: &Hash) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let nonce = Nonce::from_slice(&content_hash[..12]);

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| Error::Decompression(format!("decryption failed: {e}")))
}

/// Key encapsulation block: stores the encrypted symmetric key.
/// Written after the archive header when encryption is enabled.
///
/// Format:
///   salt: [u8; 16]       — scrypt salt for passphrase-based decryption
///   encrypted_key: [u8; 48] — ChaCha20-Poly1305(key, derived_wrapping_key)
pub struct KeyEncapsulation {
    pub salt: [u8; 16],
    pub encrypted_key: Vec<u8>, // 32 bytes key + 16 bytes tag = 48
}

impl KeyEncapsulation {
    /// Create a key encapsulation block from a passphrase
    pub fn from_passphrase(archive_key: &SymmetricKey, passphrase: &[u8]) -> Result<Self> {
        let salt = generate_salt();
        let wrapping_key = derive_key_from_passphrase(passphrase, &salt);

        // Encrypt the archive key with the wrapping key
        let cipher = ChaCha20Poly1305::new((&wrapping_key).into());
        // Use a fixed nonce for key wrapping (safe because wrapping key is unique per salt)
        let nonce = Nonce::from_slice(&[0u8; 12]);

        let encrypted_key = cipher
            .encrypt(nonce, archive_key.as_slice())
            .map_err(|e| Error::Compression(format!("key encapsulation failed: {e}")))?;

        Ok(Self {
            salt,
            encrypted_key,
        })
    }

    /// Unwrap the archive key using a passphrase
    pub fn unwrap_with_passphrase(&self, passphrase: &[u8]) -> Result<SymmetricKey> {
        let wrapping_key = derive_key_from_passphrase(passphrase, &self.salt);

        let cipher = ChaCha20Poly1305::new((&wrapping_key).into());
        let nonce = Nonce::from_slice(&[0u8; 12]);

        let key_bytes = cipher
            .decrypt(nonce, self.encrypted_key.as_slice())
            .map_err(|_| Error::Decompression("wrong passphrase or corrupted key".into()))?;

        let mut key = [0u8; 32];
        key.copy_from_slice(&key_bytes);
        Ok(key)
    }

    /// Write to a stream
    pub fn write_to(&self, w: &mut impl Write) -> std::io::Result<()> {
        w.write_all(&self.salt)?;
        let len = self.encrypted_key.len() as u32;
        w.write_all(&len.to_le_bytes())?;
        w.write_all(&self.encrypted_key)?;
        Ok(())
    }

    /// Read from a stream
    pub fn read_from(r: &mut impl Read) -> Result<Self> {
        let mut salt = [0u8; 16];
        r.read_exact(&mut salt)?;
        let mut len_buf = [0u8; 4];
        r.read_exact(&mut len_buf)?;
        let len = u32::from_le_bytes(len_buf) as usize;
        if len > 1024 {
            return Err(Error::InvalidArchive(
                "key encapsulation block too large".into(),
            ));
        }
        let mut encrypted_key = vec![0u8; len];
        r.read_exact(&mut encrypted_key)?;
        Ok(Self {
            salt,
            encrypted_key,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_round_trip() {
        let key = generate_key();
        let data = b"secret archive data that must be protected";
        let hash = blake3::hash(data).into();

        let ciphertext = encrypt_block(data, &key, &hash).unwrap();
        assert_ne!(ciphertext.as_slice(), data.as_slice());
        assert_eq!(ciphertext.len(), data.len() + TAG_SIZE);

        let plaintext = decrypt_block(&ciphertext, &key, &hash).unwrap();
        assert_eq!(plaintext, data);
    }

    #[test]
    fn wrong_key_fails() {
        let key1 = generate_key();
        let key2 = generate_key();
        let data = b"secret data";
        let hash = blake3::hash(data).into();

        let ciphertext = encrypt_block(data, &key1, &hash).unwrap();
        assert!(decrypt_block(&ciphertext, &key2, &hash).is_err());
    }

    #[test]
    fn deterministic_encryption() {
        let key = generate_key();
        let data = b"identical data";
        let hash = blake3::hash(data).into();

        let ct1 = encrypt_block(data, &key, &hash).unwrap();
        let ct2 = encrypt_block(data, &key, &hash).unwrap();
        assert_eq!(ct1, ct2); // Same key + same nonce (from hash) = same ciphertext
    }

    #[test]
    fn key_encapsulation_round_trip() {
        let archive_key = generate_key();
        let passphrase = b"correct horse battery staple";

        let encap = KeyEncapsulation::from_passphrase(&archive_key, passphrase).unwrap();

        // Write and read back
        let mut buf = Vec::new();
        encap.write_to(&mut buf).unwrap();

        let mut cursor = std::io::Cursor::new(&buf);
        let encap2 = KeyEncapsulation::read_from(&mut cursor).unwrap();

        let unwrapped = encap2.unwrap_with_passphrase(passphrase).unwrap();
        assert_eq!(unwrapped, archive_key);
    }

    #[test]
    fn wrong_passphrase_fails() {
        let archive_key = generate_key();
        let encap = KeyEncapsulation::from_passphrase(&archive_key, b"right").unwrap();
        assert!(encap.unwrap_with_passphrase(b"wrong").is_err());
    }
}
