use super::*;

#[test]
fn bundle_rejects_untrusted_kdf_costs_before_key_derivation()
-> Result<(), Box<dyn std::error::Error>> {
    let document = serde_json::json!({
        "formatVersion": 1,
        "kdf": "argon2id",
        "mCost": u32::MAX,
        "tCost": 3,
        "pCost": 4,
        "saltBase64": STANDARD.encode([0_u8; 16]),
        "ciphertextBase64": STANDARD.encode([0_u8; 32]),
        "verificationKey": {
            "algorithm": "Ed25519",
            "keyId": format!("ed25519:{}", "0".repeat(64)),
            "publicKeyBase64": STANDARD.encode([0_u8; 32])
        }
    });
    let bundle: RecoveryBundleFile = serde_json::from_value(document)?;
    assert_eq!(
        bundle.to_wrapped().err(),
        Some(RecoveryFailure::bundle_io())
    );
    Ok(())
}
