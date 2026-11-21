//! Salted iterated SHA-256 Recovery PIN hashing and constant-time verification.

use anyhow::{bail, ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;

pub const DEFAULT_ITERATIONS: u32 = 50_000;
pub const MIN_PIN_LEN: usize = 4;
pub const MAX_PIN_LEN: usize = 32;

/// Stored recovery PIN digest parameters (salt + derived hex hash).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinDigest {
    pub salt_hex: String,
    pub hash_hex: String,
    pub iterations: u32,
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn decode_hex(raw: &str) -> Result<Vec<u8>> {
    let bytes = raw.as_bytes();
    ensure!(bytes.len().is_multiple_of(2), "odd hex length");
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for &[a, b] in bytes.as_chunks::<2>().0 {
        let hi = hex_nibble(a).context("invalid hex character")?;
        let lo = hex_nibble(b).context("invalid hex character")?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

/// Draw 16 cryptographic random bytes from `/dev/urandom` for PIN salting.
///
/// # Errors
///
/// Returns an error if `/dev/urandom` cannot be opened or read.
pub fn generate_salt() -> Result<[u8; 16]> {
    let mut salt = [0u8; 16];
    let mut file = File::open("/dev/urandom").context("open /dev/urandom")?;
    file.read_exact(&mut salt).context("read random salt")?;
    Ok(salt)
}

/// Derive a lowercase SHA-256 hex digest from `pin`, `salt_hex`, and `iterations`.
///
/// # Errors
///
/// Returns an error if `iterations == 0` or `salt_hex` is not valid non-empty hex.
pub fn derive_hex(pin: &str, salt_hex: &str, iterations: u32) -> Result<String> {
    ensure!(iterations > 0, "PIN iteration count must be positive");
    let salt = decode_hex(salt_hex)?;
    ensure!(!salt.is_empty(), "PIN salt must not be empty");

    let mut h = Sha256::new();
    h.update(&salt);
    h.update(0u32.to_le_bytes());
    h.update(pin.as_bytes());
    let mut cur: [u8; 32] = h.finalize().into();

    for round in 1..iterations {
        let mut next = Sha256::new();
        next.update(cur);
        next.update(&salt);
        next.update(round.to_le_bytes());
        cur = next.finalize().into();
    }
    Ok(crate::abl::hex_lower(&cur))
}

/// Validate `pin` length (`MIN_PIN_LEN..=MAX_PIN_LEN`) and hash it with `salt`.
///
/// # Errors
///
/// Returns an error if the PIN is outside the allowed length range or contains
/// control characters, or if `iterations == 0`.
pub fn make_digest(pin: &str, salt: &[u8; 16], iterations: u32) -> Result<PinDigest> {
    let len = pin.chars().count();
    if !(MIN_PIN_LEN..=MAX_PIN_LEN).contains(&len) {
        bail!("PIN must be between {MIN_PIN_LEN} and {MAX_PIN_LEN} characters");
    }
    ensure!(!pin.chars().any(char::is_control), "PIN contains invalid control characters");
    let salt_hex = crate::abl::hex_lower(salt);
    let hash_hex = derive_hex(pin, &salt_hex, iterations)?;
    Ok(PinDigest {
        salt_hex,
        hash_hex,
        iterations,
    })
}

/// Verify `pin` against a stored salt, expected lowercase hex digest, and iteration count
/// using constant-time byte comparison.
#[must_use]
pub fn verify_pin(pin: &str, salt_hex: &str, expected_hex: &str, iterations: u32) -> bool {
    let Ok(got) = derive_hex(pin, salt_hex, iterations) else {
        return false;
    };
    let (a, b) = (got.as_bytes(), expected_hex.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_verifies_pin_in_constant_time() {
        let salt = [0x42u8; 16];
        let digest = make_digest("2468", &salt, 1000).unwrap();
        assert!(verify_pin("2468", &digest.salt_hex, &digest.hash_hex, 1000));
        assert!(!verify_pin("2469", &digest.salt_hex, &digest.hash_hex, 1000));
        assert!(!verify_pin("2468", &"11".repeat(16), &digest.hash_hex, 1000));
        assert!(!verify_pin("2468", &digest.salt_hex, &digest.hash_hex, 999));
    }

    #[test]
    fn enforces_pin_length_bounds_and_reads_urandom() {
        let salt = generate_salt().unwrap();
        assert_eq!(salt.len(), 16);
        assert!(make_digest("123", &salt, 10).is_err());
        assert!(make_digest(&"9".repeat(MAX_PIN_LEN + 1), &salt, 10).is_err());
        assert!(make_digest("1234", &salt, 10).is_ok());
    }
}
