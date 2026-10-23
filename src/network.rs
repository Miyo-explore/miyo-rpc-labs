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
    ("testnet", "Test SDF Network ; February 2023"),
    ("mainnet", "Public Global Stellar Network ; September 2015"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_passphrases() {
        assert_eq!(
            network_passphrase("testnet"),
            Some("Test SDF Network ; February 2023")
        );
        assert_eq!(
            network_passphrase("mainnet"),
            Some("Public Global Stellar Network ; September 2015")
        );
    }

    #[test]
    fn test_case_insensitive_lookup() {
        assert_eq!(
            network_passphrase("TESTNET"),
            Some("Test SDF Network ; February 2023")
        );
        assert_eq!(
            network_passphrase("MainNet"),
            Some("Public Global Stellar Network ; September 2015")
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
    }
}
