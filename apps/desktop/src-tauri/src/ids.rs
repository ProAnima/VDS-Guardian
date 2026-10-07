//! Fresh identifiers for records the desktop app creates (profiles, credentials, repositories,
//! plans, safety runs): `<prefix>-` followed by 128 bits from the operating-system RNG in hex.

use rand_core::{OsRng, RngCore};

pub(crate) fn random_id(prefix: &str) -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    let suffix = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{prefix}-{suffix}")
}

#[cfg(test)]
mod tests {
    use super::random_id;

    #[test]
    fn ids_carry_the_prefix_and_128_random_bits_in_hex() {
        let first = random_id("plan");
        let suffix = first.strip_prefix("plan-").unwrap_or_default();
        assert_eq!(suffix.len(), 32);
        assert!(suffix.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(first, random_id("plan"));
    }
}
