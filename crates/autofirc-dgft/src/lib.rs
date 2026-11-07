//! DGFT Open API integration: PBKDF2 credential derivation, RSA-SHA256
//! payload signing, IRM fetch/match, eBRC self-certification — plus a
//! high-fidelity mock server for development and demo (the real API needs
//! an IEC, IP whitelisting, and FIU registration).
pub mod client;
pub mod crypto;
pub mod mock;
pub mod portal;
pub mod types;
