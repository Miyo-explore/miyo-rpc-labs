//! Stellar network identity helpers.
//!
//! Provides known network passphrases so the CLI can validate that an
//! endpoint's `getNetwork` response matches the expected network (for example
//! via the `--expected-network` flag).
//!
//! Network passphrases are public, protocol-level constants that change only
//! on a network upgrade, so they are safe to embed.

/// Known Stellar network passphrases, in canonical `(name, passphrase)` order.
pub static NETWORK_PASSPHRASES: &[(&str, &str)] = &[
    ("testnet", "Test SDF Network ; September 2015"),
    ("mainnet", "Public Global Stellar Network ; September 2015"),
    ("futurenet", "Test SDF Future Network ; October 2022"),
];

/// Look up the passphrase for a known network name (case-insensitive).
pub fn network_passphrase(name: &str) -> Option<&'static str> {
    NETWORK_PASSPHRASES
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, passphrase)| *passphrase)
}

/// Return the names of all known networks.
pub fn known_networks() -> Vec<&'static str> {
    NETWORK_PASSPHRASES.iter().map(|(n, _)| *n).collect()
}

/// Version byte of a Stellar contract address strkey (`C...`).
///
/// Strkeys encode the version in the first 5 bits of the first base32
/// character: 'C' is base32 index 2, so the version byte is `2 << 3`.
/// (Muxed accounts share the 'M' prefix at version `12 << 3`; a contract
/// strkey is distinguished by its 56-character length / 32-byte payload.)
pub const CONTRACT_STRKEY_VERSION_BYTE: u8 = 2 << 3;

/// CRC-16/XMODEM (poly `0x1021`, init `0x0000`, no reflection, no xor-out),
/// the checksum Stellar strkeys carry over `[version_byte || payload]`,
/// stored little-endian.
fn crc16_xmodem(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

const BASE32_ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// Decode RFC 4648 base32 (uppercase, no padding) into bytes.
///
/// Returns `None` on any character outside the alphabet or on a
/// non-canonical trailing bit pattern.
fn base32_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 5 / 8);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for c in input.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'2'..=b'7' => c - b'2' + 26,
            _ => return None,
        } as u32;
        acc = (acc << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    // Any leftover bits must be zero (canonical encoding); a leftover of
    // 5+ bits means the input length cannot encode whole bytes.
    if bits >= 5 || (bits > 0 && (acc & ((1u32 << bits) - 1)) != 0) {
        return None;
    }
    Some(out)
}

/// Encode bytes as RFC 4648 base32 (uppercase, no padding).
fn base32_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() * 8).div_ceil(5));
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for &b in data {
        acc = (acc << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(BASE32_ALPHABET[((acc >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(BASE32_ALPHABET[((acc << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// Decode a contract address strkey (`C...`, 56 characters) into its
/// 32-byte contract ID.
///
/// Verifies the length, the `C` prefix, the version byte, and the
/// CRC-16/XMODEM checksum. Returns a human-readable reason on failure —
/// the caller reports it as a usage error (exit code 2) before any
/// network request is made.
pub fn decode_contract_id(strkey: &str) -> Result<[u8; 32], String> {
    if strkey.len() != 56 {
        return Err(format!(
            "contract ID must be a 56-character strkey (got {} characters)",
            strkey.len()
        ));
    }
    if !strkey.starts_with('C') {
        return Err("contract ID must start with 'C'".to_string());
    }
    let raw = base32_decode(strkey).ok_or_else(|| "contract ID is not valid base32".to_string())?;
    if raw.len() != 35 {
        return Err(format!(
            "contract ID decodes to {} bytes, expected 35 (version + 32-byte ID + 2-byte checksum)",
            raw.len()
        ));
    }
    if raw[0] != CONTRACT_STRKEY_VERSION_BYTE {
        return Err(format!(
            "contract ID has unexpected version byte 0x{:02x}",
            raw[0]
        ));
    }
    let stored = u16::from_le_bytes([raw[33], raw[34]]);
    if crc16_xmodem(&raw[..33]) != stored {
        return Err("contract ID checksum mismatch".to_string());
    }
    let mut id = [0u8; 32];
    id.copy_from_slice(&raw[1..33]);
    Ok(id)
}

/// Encode a 32-byte contract ID as a `C...` strkey.
pub fn encode_contract_id(contract_id: &[u8; 32]) -> String {
    let mut raw = Vec::with_capacity(35);
    raw.push(CONTRACT_STRKEY_VERSION_BYTE);
    raw.extend_from_slice(contract_id);
    raw.extend_from_slice(&crc16_xmodem(&raw).to_le_bytes());
    base32_encode(&raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_passphrases() {
        assert_eq!(
            network_passphrase("testnet"),
            Some("Test SDF Network ; September 2015")
        );
        assert_eq!(
            network_passphrase("mainnet"),
            Some("Public Global Stellar Network ; September 2015")
        );
        assert_eq!(
            network_passphrase("futurenet"),
            Some("Test SDF Future Network ; October 2022")
        );
    }

    #[test]
    fn test_case_insensitive_lookup() {
        assert_eq!(
            network_passphrase("TESTNET"),
            Some("Test SDF Network ; September 2015")
        );
        assert_eq!(
            network_passphrase("MainNet"),
            Some("Public Global Stellar Network ; September 2015")
        );
        assert_eq!(
            network_passphrase("FutureNet"),
            Some("Test SDF Future Network ; October 2022")
        );
    }

    #[test]
    fn test_unknown_returns_none() {
        assert_eq!(network_passphrase("foo"), None);
        assert_eq!(network_passphrase(""), None);
    }

    #[test]
    fn test_known_networks_listed() {
        let nets = known_networks();
        assert!(nets.contains(&"testnet"));
        assert!(nets.contains(&"mainnet"));
        assert!(nets.contains(&"futurenet"));
    }

    #[test]
    fn test_contract_strkey_round_trip_live_ids() {
        // Real Testnet contracts, verified live 2026-11-27: the native
        // XLM Stellar Asset Contract and a Wasm contract found via
        // getEvents. The SAC payload is the contract ID decoded from the
        // strkey by an independent Python check (base32 + CRC-16/XMODEM).
        let sac = "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC";
        let sac_id = decode_contract_id(sac).expect("SAC strkey must decode");
        assert_eq!(
            sac_id,
            [
                0xd7, 0x92, 0x8b, 0x72, 0xc2, 0x70, 0x3c, 0xcf, 0xea, 0xf7, 0xeb, 0x9f, 0xf4, 0xef,
                0x4d, 0x50, 0x4a, 0x55, 0xa8, 0xb9, 0x79, 0xfc, 0x9b, 0x45, 0x0e, 0xa2, 0xc8, 0x42,
                0xb4, 0xd1, 0xce, 0x61
            ]
        );
        assert_eq!(encode_contract_id(&sac_id), sac);

        let wasm = "CBSOWXRDBW4BZ2WYGQWJN25U24BDXOTLAS4U7NURAMDTTBGB72CNSP5Y";
        let wasm_id = decode_contract_id(wasm).expect("Wasm contract strkey must decode");
        assert_eq!(encode_contract_id(&wasm_id), wasm);
    }

    #[test]
    fn test_contract_strkey_rejects_bad_input() {
        // Wrong length.
        assert!(
            decode_contract_id("CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYS").is_err()
        );
        // Wrong prefix (a G... account strkey of the right length).
        assert!(
            decode_contract_id("GDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC").is_err()
        );
        // Bad checksum (last character changed).
        assert!(
            decode_contract_id("CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSD").is_err()
        );
        // Invalid base32 character.
        assert!(
            decode_contract_id("CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYS1").is_err()
        );
        // Empty.
        assert!(decode_contract_id("").is_err());
    }

    #[test]
    fn test_base32_round_trip() {
        for bytes in [vec![], vec![0u8], vec![0xff; 32], (0u8..=255).collect()] {
            assert_eq!(base32_decode(&base32_encode(&bytes)).unwrap(), bytes);
        }
        // The all-zero 35-byte strkey body (56 base32 chars) decodes cleanly.
        let zeros = base32_encode(&[0u8; 35]);
        assert_eq!(zeros.len(), 56);
        assert_eq!(base32_decode(&zeros).unwrap().len(), 35);
    }
}
