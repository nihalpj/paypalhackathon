use base64::{Engine, engine::general_purpose::STANDARD as B64};
use rand::RngCore;
use rsa::RsaPrivateKey;
use rsa::pkcs1v15::SigningKey;
use rsa::signature::{RandomizedSigner, SignatureEncoding};
use sha2::{Digest, Sha256};

/// PBKDF2 parameters mandated by the DGFT Open API spec.
pub const PBKDF2_ITERATIONS: u32 = 65_536;
pub const PBKDF2_KEY_BYTES: usize = 32; // 256-bit
pub const SALT_BYTES: usize = 32;

/// Derived DGFT credential: random 32-byte salt, SHA-256 pre-hash of the
/// client secret, then PBKDF2-HMAC-SHA256 at 65,536 iterations.
#[derive(Debug, Clone, PartialEq)]
pub struct DerivedSecret {
    pub salt_b64: String,
    pub hash_b64: String,
}

pub fn derive_client_secret(client_secret: &str) -> DerivedSecret {
    let mut salt = [0u8; SALT_BYTES];
    rand::thread_rng().fill_bytes(&mut salt);
    derive_with_salt(client_secret, &salt)
}

/// Deterministic variant (testing / replay): PBKDF2 over SHA-256(secret)
/// with a caller-supplied salt.
pub fn derive_with_salt(client_secret: &str, salt: &[u8]) -> DerivedSecret {
    let prehashed = Sha256::digest(client_secret.as_bytes());
    let mut key = [0u8; PBKDF2_KEY_BYTES];
    pbkdf2::pbkdf2_hmac::<Sha256>(&prehashed, salt, PBKDF2_ITERATIONS, &mut key);
    DerivedSecret {
        salt_b64: B64.encode(salt),
        hash_b64: B64.encode(key),
    }
}

/// RSA-SHA256 (PKCS#1 v1.5) signature over the canonical JSON payload.
pub fn sign_payload(private_key: &RsaPrivateKey, payload: &serde_json::Value) -> String {
    let canonical = serde_json::to_vec(payload).expect("payload serializes");
    let signing = SigningKey::<Sha256>::new(private_key.clone());
    let mut rng = rand::thread_rng();
    B64.encode(signing.sign_with_rng(&mut rng, &canonical).to_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pbkdf2_derivation_is_deterministic_per_salt() {
        let a = derive_with_salt("secret-123", b"fixed-salt-32bytes-padded-00000000");
        let b = derive_with_salt("secret-123", b"fixed-salt-32bytes-padded-00000000");
        assert_eq!(a, b);
        // 256-bit key, base64-encoded => 44 chars
        assert_eq!(a.hash_b64.len(), 44);
        // a different secret yields a different hash
        let c = derive_with_salt("secret-456", b"fixed-salt-32bytes-padded-00000000");
        assert_ne!(a.hash_b64, c.hash_b64);
    }

    #[test]
    fn random_derivation_uses_random_salts() {
        let a = derive_client_secret("secret-123");
        let b = derive_client_secret("secret-123");
        assert_ne!(a.salt_b64, b.salt_b64);
        assert_ne!(a.hash_b64, b.hash_b64);
    }

    #[test]
    fn signature_verifies_against_public_key() {
        use rsa::pkcs1v15::VerifyingKey;
        use rsa::signature::Verifier;
        let mut rng = rand::thread_rng();
        let private = rsa::RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let public = rsa::RsaPublicKey::from(&private);
        let payload = serde_json::json!({"iec": "0123456789", "irm": "IRM-1"});
        let sig_b64 = sign_payload(&private, &payload);
        let sig_bytes = B64.decode(sig_b64).unwrap();
        let sig = rsa::pkcs1v15::Signature::try_from(sig_bytes.as_slice()).unwrap();
        let canonical = serde_json::to_vec(&payload).unwrap();
        assert!(
            VerifyingKey::<Sha256>::new(public)
                .verify(&canonical, &sig)
                .is_ok()
        );
    }
}
