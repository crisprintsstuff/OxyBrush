//! Checking downloads before anything is installed.
//!
//! Every release carries `SHA256SUMS.txt` (written by the release
//! workflow). A downloaded file must match its line there. When the build
//! has a signing public key (`crate::SIGNING_PUBLIC_KEY`), the checksum file
//! itself must also carry a valid ed25519 signature (`SHA256SUMS.txt.sig`,
//! hex), so a compromised download host alone can't ship a fake update.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

/// Parse `sha256sum` output: `<64 hex>  <name>` (or ` *<name>` in binary mode).
pub fn parse_checksums(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let (hash, name) = line.trim().split_once(char::is_whitespace)?;
            let name = name.trim_start().trim_start_matches('*');
            (hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) && !name.is_empty())
                .then(|| (name.to_owned(), hash.to_ascii_lowercase()))
        })
        .collect()
}

/// SHA-256 of a file, lowercase hex.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(to_hex(&hasher.finalize()))
}

/// Check `path` (published as `name`) against the checksum list.
pub fn verify_file(path: &Path, name: &str, checksums: &str) -> Result<(), String> {
    let sums = parse_checksums(checksums);
    let expected = sums.get(name).ok_or_else(|| format!("{name} is not listed in SHA256SUMS.txt"))?;
    let actual = sha256_file(path).map_err(|e| format!("could not read the download: {e}"))?;
    if &actual == expected {
        Ok(())
    } else {
        Err(format!("checksum mismatch for {name}: the download is damaged or was altered"))
    }
}

/// Check the signature over the checksum list.
pub fn verify_signature(checksums: &[u8], signature_hex: &str, public_key_hex: &str) -> Result<(), String> {
    let key: [u8; 32] = from_hex(public_key_hex).and_then(|b| b.try_into().ok()).ok_or("invalid public key")?;
    let key = VerifyingKey::from_bytes(&key).map_err(|_| "invalid public key")?;
    let sig: [u8; 64] =
        from_hex(signature_hex.trim()).and_then(|b| b.try_into().ok()).ok_or("signature is not 64 hex bytes")?;
    key.verify(checksums, &Signature::from_bytes(&sig))
        .map_err(|_| "signature does not match: not an official release".into())
}

/// Sign the checksum list (release workflow). `seed_hex` is the 32-byte secret key.
pub fn sign(checksums: &[u8], seed_hex: &str) -> Result<String, String> {
    let seed: [u8; 32] =
        from_hex(seed_hex.trim()).and_then(|b| b.try_into().ok()).ok_or("secret key must be 32 hex bytes")?;
    Ok(to_hex(&SigningKey::from_bytes(&seed).sign(checksums).to_bytes()))
}

/// A new key pair: (secret seed hex, public key hex).
pub fn generate_key() -> Result<(String, String), String> {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|e| format!("no system randomness: {e}"))?;
    let key = SigningKey::from_bytes(&seed);
    Ok((to_hex(&seed), to_hex(key.verifying_key().as_bytes())))
}

/// Public key for a secret seed.
pub fn public_key(seed_hex: &str) -> Result<String, String> {
    let seed: [u8; 32] =
        from_hex(seed_hex.trim()).and_then(|b| b.try_into().ok()).ok_or("secret key must be 32 hex bytes")?;
    Ok(to_hex(SigningKey::from_bytes(&seed).verifying_key().as_bytes()))
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_lines_and_file_verification() {
        let dir = std::env::temp_dir().join(format!("oxy-update-verify-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("OxyBrush-0.4.0-alpha-linux-x86_64.tar.gz");
        std::fs::write(&file, b"hello").unwrap();
        // sha256("hello")
        let good = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let sums =
            format!("{good}  OxyBrush-0.4.0-alpha-linux-x86_64.tar.gz\nnot a line\n{} *other.zip\n", "0".repeat(64));
        assert_eq!(parse_checksums(&sums).len(), 2);
        assert!(verify_file(&file, "OxyBrush-0.4.0-alpha-linux-x86_64.tar.gz", &sums).is_ok());
        assert!(verify_file(&file, "missing.dmg", &sums).unwrap_err().contains("not listed"));
        std::fs::write(&file, b"hellO").unwrap();
        assert!(
            verify_file(&file, "OxyBrush-0.4.0-alpha-linux-x86_64.tar.gz", &sums).unwrap_err().contains("mismatch")
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn signatures_round_trip_and_reject_tampering() {
        let (secret, public) = generate_key().unwrap();
        assert_eq!(public_key(&secret).unwrap(), public);
        let sums = b"abc  file\n";
        let sig = sign(sums, &secret).unwrap();
        assert!(verify_signature(sums, &sig, &public).is_ok());
        assert!(verify_signature(b"abd  file\n", &sig, &public).is_err(), "tampered list");
        let (_, other) = generate_key().unwrap();
        assert!(verify_signature(sums, &sig, &other).is_err(), "wrong key");
        assert!(verify_signature(sums, "zz", &public).is_err());
        assert!(sign(sums, "short").is_err());
    }
}
