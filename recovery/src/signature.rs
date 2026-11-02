//! IR Blaster firmware signatures (firmware repository: tools/sign_firmware.py).
//!
//! A signed file is the image followed by a 72-byte trailer: "IRBSIG1\0" and an Ed25519 signature
//! of "irblaster-firmware-v1\0" + product + "\0" + chip + "\0" + SHA-256(image). Binding the product
//! and chip means a validly signed node image is still refused for a hub, or for another chip.

use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

const MAGIC: &[u8; 8] = b"IRBSIG1\0";
const TRAILER_LEN: usize = 72;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Signed by a trusted key for this product and chip.
    Valid,
    /// Carries a trailer, but not a valid signature from a trusted key for this product and chip.
    Invalid,
    /// No signature trailer at all.
    Unsigned,
    /// No trusted keys are configured, so signatures can't be checked.
    NotChecked,
}

pub fn trusted_keys() -> Vec<VerifyingKey> {
    parse_keys(include_str!("../trusted_keys.txt"))
}

fn parse_keys(text: &str) -> Vec<VerifyingKey> {
    text.lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let bytes = decode_hex(line)?;
            let array: [u8; 32] = bytes.try_into().ok()?;
            VerifyingKey::from_bytes(&array).ok()
        })
        .collect()
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

pub fn is_signed(data: &[u8]) -> bool {
    data.len() > TRAILER_LEN
        && &data[data.len() - TRAILER_LEN..data.len() - TRAILER_LEN + MAGIC.len()] == MAGIC
}

fn message(product: &str, chip: &str, image: &[u8]) -> Vec<u8> {
    let mut msg = b"irblaster-firmware-v1\0".to_vec();
    msg.extend_from_slice(product.as_bytes());
    msg.push(0);
    msg.extend_from_slice(chip.as_bytes());
    msg.push(0);
    msg.extend_from_slice(&Sha256::digest(image));
    msg
}

pub fn check(product: &str, chip: &str, data: &[u8]) -> Verdict {
    check_with(&trusted_keys(), product, chip, data)
}

fn check_with(keys: &[VerifyingKey], product: &str, chip: &str, data: &[u8]) -> Verdict {
    if keys.is_empty() {
        return Verdict::NotChecked;
    }
    if !is_signed(data) {
        return Verdict::Unsigned;
    }
    let (image, trailer) = data.split_at(data.len() - TRAILER_LEN);
    let signature_bytes: [u8; 64] = trailer[MAGIC.len()..]
        .try_into()
        .expect("trailer is 72 bytes");
    let signature = Signature::from_bytes(&signature_bytes);
    let msg = message(product, chip, image);
    if keys
        .iter()
        .any(|key| key.verify_strict(&msg, &signature).is_ok())
    {
        Verdict::Valid
    } else {
        Verdict::Invalid
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn signed(key: &SigningKey, product: &str, chip: &str, image: &[u8]) -> Vec<u8> {
        let mut out = image.to_vec();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&key.sign(&message(product, chip, image)).to_bytes());
        out
    }

    #[test]
    fn verifies_product_and_chip_binding() {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let keys = [key.verifying_key()];
        let image = b"\xE9 pretend this is firmware";
        let file = signed(&key, "node", "esp32", image);
        assert_eq!(check_with(&keys, "node", "esp32", &file), Verdict::Valid);
        assert_eq!(check_with(&keys, "hub", "esp32", &file), Verdict::Invalid);
        assert_eq!(
            check_with(&keys, "node", "esp32c3", &file),
            Verdict::Invalid
        );
        assert_eq!(check_with(&keys, "node", "esp32", image), Verdict::Unsigned);
        assert_eq!(check_with(&[], "node", "esp32", &file), Verdict::NotChecked);

        let other = SigningKey::from_bytes(&[9u8; 32]);
        assert_eq!(
            check_with(&[other.verifying_key()], "node", "esp32", &file),
            Verdict::Invalid
        );

        let mut tampered = file.clone();
        tampered[3] ^= 1;
        assert_eq!(
            check_with(&keys, "node", "esp32", &tampered),
            Verdict::Invalid
        );
    }

    #[test]
    fn parses_key_lists() {
        let key = SigningKey::from_bytes(&[7u8; 32]).verifying_key();
        let hex: String = key.to_bytes().iter().map(|b| format!("{b:02x}")).collect();
        let text = format!("# comment\n\n{hex}  # signing key 1\nnot-hex\n");
        assert_eq!(parse_keys(&text), vec![key]);
        assert!(parse_keys(include_str!("../trusted_keys.txt")).len() <= 16);
    }
}
