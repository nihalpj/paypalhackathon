use base64::{Engine, engine::general_purpose::STANDARD as B64};
use rsa::RsaPublicKey;
use rsa::pkcs1v15::{Signature, VerifyingKey};
use rsa::signature::Verifier;
use sha2::Sha256;
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum WebhookError {
    #[error("untrusted certificate url host: {0}")]
    UntrustedCertUrl(String),
    #[error("invalid base64 signature")]
    BadSignatureEncoding,
    #[error("signature verification failed")]
    VerificationFailed,
    #[error("certificate fetch/parse failed: {0}")]
    Cert(String),
}

/// Headers PayPal sends with every webhook delivery.
#[derive(Debug, Clone)]
pub struct TransmissionMeta {
    pub transmission_id: String,
    pub transmission_time: String,
    pub cert_url: String,
    pub transmission_sig: String,
}

/// CRC32 of the *raw* request body, as an unsigned 32-bit integer.
pub fn body_crc32(raw_body: &[u8]) -> u32 {
    crc32fast::hash(raw_body)
}

/// The exact pipe-delimited message PayPal signs.
pub fn signed_message(meta: &TransmissionMeta, webhook_id: &str, raw_body: &[u8]) -> String {
    format!(
        "{}|{}|{}|{}",
        meta.transmission_id,
        meta.transmission_time,
        webhook_id,
        body_crc32(raw_body)
    )
}

/// Only certificates served from paypal.com (or its subdomains) are trusted.
pub fn is_trusted_cert_url(cert_url: &str) -> bool {
    let Ok(url) = Url::parse(cert_url) else {
        return false;
    };
    if url.scheme() != "https" {
        return false;
    }
    match url.host_str() {
        Some("paypal.com") => true,
        Some(host) => host.ends_with(".paypal.com"),
        None => false,
    }
}

/// Verify the RSA-SHA256 signature over the signed message.
pub fn verify_signature(
    meta: &TransmissionMeta,
    webhook_id: &str,
    raw_body: &[u8],
    public_key: &RsaPublicKey,
) -> Result<(), WebhookError> {
    let sig_bytes = B64
        .decode(&meta.transmission_sig)
        .map_err(|_| WebhookError::BadSignatureEncoding)?;
    let signature =
        Signature::try_from(sig_bytes.as_slice()).map_err(|_| WebhookError::BadSignatureEncoding)?;
    let message = signed_message(meta, webhook_id, raw_body);
    VerifyingKey::<Sha256>::new(public_key.clone())
        .verify(message.as_bytes(), &signature)
        .map_err(|_| WebhookError::VerificationFailed)
}

/// Fetch the signing certificate from a trusted URL and extract its RSA key.
pub async fn fetch_public_key(
    http: &reqwest::Client,
    cert_url: &str,
) -> Result<RsaPublicKey, WebhookError> {
    if !is_trusted_cert_url(cert_url) {
        let host = Url::parse(cert_url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_else(|| cert_url.to_owned());
        return Err(WebhookError::UntrustedCertUrl(host));
    }
    let pem = http
        .get(cert_url)
        .send()
        .await
        .map_err(|e| WebhookError::Cert(e.to_string()))?
        .text()
        .await
        .map_err(|e| WebhookError::Cert(e.to_string()))?;
    public_key_from_pem(&pem)
}

/// Parse a PEM X.509 certificate and return its RSA public key.
pub fn public_key_from_pem(pem: &str) -> Result<RsaPublicKey, WebhookError> {
    use rsa::pkcs8::DecodePublicKey;
    let (_, pem_block) = x509_parser::pem::parse_x509_pem(pem.as_bytes())
        .map_err(|e| WebhookError::Cert(format!("pem parse: {e}")))?;
    let (_, cert) = x509_parser::parse_x509_certificate(&pem_block.contents)
        .map_err(|e| WebhookError::Cert(format!("x509 parse: {e}")))?;
    let spki = cert.public_key();
    RsaPublicKey::from_public_key_der(spki.raw).map_err(|e| WebhookError::Cert(format!("spki der: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::pkcs1v15::SigningKey;
    use rsa::signature::{RandomizedSigner, SignatureEncoding};
    use rsa::traits::PublicKeyParts;

    #[test]
    fn crc32_matches_known_vector() {
        // CRC-32/ISO-HDLC of "123456789" is 0xCBF43926.
        assert_eq!(body_crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn cert_url_allowlist() {
        assert!(is_trusted_cert_url("https://api.paypal.com/cert.pem"));
        assert!(is_trusted_cert_url("https://api.sandbox.paypal.com/cert.pem"));
        assert!(!is_trusted_cert_url("http://api.paypal.com/cert.pem")); // no http
        assert!(!is_trusted_cert_url("https://paypal.com.evil.example/cert.pem"));
        assert!(!is_trusted_cert_url("https://evil-paypal.com/cert.pem"));
        assert!(!is_trusted_cert_url("not a url"));
    }

    #[test]
    fn signed_message_and_verification_round_trip() {
        let mut rng = rand::thread_rng();
        let private = rsa::RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let public = rsa::RsaPublicKey::from(&private);

        let meta = TransmissionMeta {
            transmission_id: "tid-1".into(),
            transmission_time: "2026-11-04T10:00:00Z".into(),
            cert_url: "https://api.paypal.com/cert.pem".into(),
            transmission_sig: String::new(),
        };
        let body = br#"{"id":"WH-1","event_type":"PAYMENT.CAPTURE.COMPLETED"}"#;
        let message = signed_message(&meta, "WEBHOOK-ID-42", body);
        assert!(message.starts_with("tid-1|2026-11-04T10:00:00Z|WEBHOOK-ID-42|"));

        let signing = SigningKey::<Sha256>::new(private);
        let sig = signing.sign_with_rng(&mut rng, message.as_bytes());
        let meta = TransmissionMeta {
            transmission_sig: B64.encode(sig.to_bytes()),
            ..meta
        };
        assert!(verify_signature(&meta, "WEBHOOK-ID-42", body, &public).is_ok());

        // wrong webhook id -> different message -> must fail
        assert!(matches!(
            verify_signature(&meta, "WRONG", body, &public),
            Err(WebhookError::VerificationFailed)
        ));
        // tampered body -> different crc32 -> must fail
        assert!(matches!(
            verify_signature(&meta, "WEBHOOK-ID-42", b"{\"tampered\":true}", &public),
            Err(WebhookError::VerificationFailed)
        ));
    }

    #[tokio::test]
    async fn untrusted_cert_url_is_rejected_before_network() {
        let http = reqwest::Client::new();
        let err = fetch_public_key(&http, "https://evil.example.com/cert.pem")
            .await
            .unwrap_err();
        assert!(matches!(err, WebhookError::UntrustedCertUrl(_)));
    }

    #[test]
    fn parses_rsa_key_from_real_pem_fixture() {
        // Generated with openssl (see tests/fixtures); key must round-trip
        // through x509-parser.
        let pem = include_str!("../tests/fixtures/test_cert.pem");
        let key = public_key_from_pem(pem).expect("fixture cert parses");
        assert_eq!(key.size(), 256); // 2048-bit
    }
}
