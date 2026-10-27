//! Server-side cryptography (TRD §7.4, §7.6).
//!
//! - [`Hasher`]: keyed HMAC-SHA-256 with domain separation. Used wherever a value must be
//!   looked up or compared without being stored: phone numbers in the OTP table, OTP codes,
//!   refresh tokens, staff session tokens, national ID numbers (duplicate-account check).
//! - [`Sealer`]: AES-256-GCM for the 🔒 columns (date of birth, national ID, emergency-contact
//!   phones, payout details, staff TOTP secrets). Output is `version || nonce || ciphertext`,
//!   bound to its column through associated data so a value cannot be moved to another column.
//! - [`TokenSigner`]: Ed25519 JWTs (`alg: EdDSA`) for 15-minute access tokens.
//!
//! Each key is derived from a secret string of at least 32 random characters held in the
//! secret manager, so operators never handle raw key bytes. Envelope encryption with cloud
//! KMS data keys replaces the static data key once the cloud provider is chosen; the version
//! byte makes that migration possible without rewriting old values first.

use aes_gcm::{
    Aes256Gcm,
    aead::{Aead, KeyInit, Payload},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hmac::{Hmac, Mac};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

type HmacSha256 = Hmac<Sha256>;

/// 32 bytes of key material derived from a configured secret and a purpose label, so one
/// secret can never be reused as two different keys by accident.
fn derive_key(secret: &SecretString, purpose: &str) -> Zeroizing<[u8; 32]> {
    let mut h = Sha256::new();
    h.update(b"fuber-key-v1\0");
    h.update(purpose.as_bytes());
    h.update(b"\0");
    h.update(secret.expose_secret().as_bytes());
    Zeroizing::new(h.finalize().into())
}

/// Cryptographically random bytes from the operating system.
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    rand::fill(&mut b);
    b
}

/// A 256-bit random token, base64url without padding (43 characters).
pub fn random_token() -> String {
    B64.encode(random_bytes::<32>())
}

/// Constant-time equality for secrets (OTP and token hashes).
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

// ---------------------------------------------------------------------------------------------

#[derive(Clone)]
pub struct Hasher {
    key: Zeroizing<[u8; 32]>,
}

impl Hasher {
    pub fn new(secret: &SecretString) -> Self {
        Hasher { key: derive_key(secret, "hash") }
    }

    /// HMAC-SHA-256 of `data` under a domain label such as `"phone"` or `"refresh"`.
    pub fn hash(&self, domain: &str, data: &[u8]) -> [u8; 32] {
        let mut mac = HmacSha256::new_from_slice(self.key.as_ref()).expect("HMAC accepts any key length");
        mac.update(domain.as_bytes());
        mac.update(b"\0");
        mac.update(data);
        mac.finalize().into_bytes().into()
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum SealError {
    #[error("sealed value is malformed")]
    Malformed,
    #[error("sealed value failed authentication")]
    Authentication,
}

const SEAL_VERSION: u8 = 1;
const NONCE_LEN: usize = 12;

#[derive(Clone)]
pub struct Sealer {
    cipher: Aes256Gcm,
}

impl Sealer {
    pub fn new(secret: &SecretString) -> Self {
        let key = derive_key(secret, "seal");
        Sealer { cipher: Aes256Gcm::new_from_slice(key.as_ref()).expect("32-byte key") }
    }

    /// Encrypts `plaintext` for the column named by `aad` (e.g. `"users.date_of_birth"`).
    pub fn seal(&self, aad: &str, plaintext: &[u8]) -> Vec<u8> {
        let nonce = random_bytes::<NONCE_LEN>();
        let ct = self
            .cipher
            .encrypt(&nonce.into(), Payload { msg: plaintext, aad: aad.as_bytes() })
            .expect("AES-GCM encryption cannot fail for in-memory buffers");
        let mut out = Vec::with_capacity(1 + NONCE_LEN + ct.len());
        out.push(SEAL_VERSION);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        out
    }

    pub fn open(&self, aad: &str, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, SealError> {
        let (&version, rest) = sealed.split_first().ok_or(SealError::Malformed)?;
        if version != SEAL_VERSION || rest.len() < NONCE_LEN + 16 {
            return Err(SealError::Malformed);
        }
        let (nonce, ct) = rest.split_at(NONCE_LEN);
        let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| SealError::Malformed)?;
        self.cipher
            .decrypt(&nonce.into(), Payload { msg: ct, aad: aad.as_bytes() })
            .map(Zeroizing::new)
            .map_err(|_| SealError::Authentication)
    }

    pub fn seal_str(&self, aad: &str, plaintext: &str) -> Vec<u8> {
        self.seal(aad, plaintext.as_bytes())
    }

    pub fn open_str(&self, aad: &str, sealed: &[u8]) -> Result<String, SealError> {
        let bytes = self.open(aad, sealed)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| SealError::Malformed)
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TokenError {
    #[error("token is malformed")]
    Malformed,
    #[error("token signature or key id is not valid")]
    BadSignature,
    #[error("token has expired")]
    Expired,
}

#[derive(Serialize, Deserialize)]
struct Header<'a> {
    alg: &'a str,
    typ: &'a str,
    kid: &'a str,
}

/// Signs and verifies compact JWS tokens with Ed25519. Only `alg: EdDSA` and the configured
/// key ids are accepted; `alg: none` and algorithm-confusion attacks are rejected by design,
/// because the algorithm is never read from the token to pick a verifier.
#[derive(Clone)]
pub struct TokenSigner {
    kid: String,
    signing: SigningKey,
    /// Keys accepted for verification: the current one plus any still-valid previous keys
    /// during a 90-day rotation overlap.
    verifying: Vec<(String, VerifyingKey)>,
}

impl TokenSigner {
    pub fn new(kid: &str, secret: &SecretString, previous: &[(String, SecretString)]) -> Self {
        let signing = SigningKey::from_bytes(&derive_key(secret, "jwt-ed25519"));
        let mut verifying = vec![(kid.to_owned(), signing.verifying_key())];
        for (k, s) in previous {
            verifying.push((k.clone(), SigningKey::from_bytes(&derive_key(s, "jwt-ed25519")).verifying_key()));
        }
        TokenSigner { kid: kid.to_owned(), signing, verifying }
    }

    pub fn sign<C: Serialize>(&self, claims: &C) -> String {
        let header = Header { alg: "EdDSA", typ: "JWT", kid: &self.kid };
        let h = B64.encode(serde_json::to_vec(&header).expect("header serialises"));
        let p = B64.encode(serde_json::to_vec(claims).expect("claims serialise"));
        let signing_input = format!("{h}.{p}");
        let sig: Signature = self.signing.sign(signing_input.as_bytes());
        format!("{signing_input}.{}", B64.encode(sig.to_bytes()))
    }

    /// Verifies the signature and `exp` (seconds since the epoch, compared with `now`).
    pub fn verify<C: DeserializeOwned + HasExpiry>(&self, token: &str, now: i64) -> Result<C, TokenError> {
        if token.len() > 2048 {
            return Err(TokenError::Malformed);
        }
        let mut parts = token.split('.');
        let (Some(h), Some(p), Some(s), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
            return Err(TokenError::Malformed);
        };
        let header_bytes = B64.decode(h).map_err(|_| TokenError::Malformed)?;
        let header: Header = serde_json::from_slice(&header_bytes).map_err(|_| TokenError::Malformed)?;
        if header.alg != "EdDSA" || header.typ != "JWT" {
            return Err(TokenError::BadSignature);
        }
        let key = self.verifying.iter().find(|(k, _)| k == header.kid).map(|(_, v)| v).ok_or(TokenError::BadSignature)?;
        let sig_bytes: [u8; 64] = B64.decode(s).map_err(|_| TokenError::Malformed)?.try_into().map_err(|_| TokenError::Malformed)?;
        let signing_input_len = h.len() + 1 + p.len();
        key.verify_strict(&token.as_bytes()[..signing_input_len], &Signature::from_bytes(&sig_bytes))
            .map_err(|_| TokenError::BadSignature)?;
        let claims: C = serde_json::from_slice(&B64.decode(p).map_err(|_| TokenError::Malformed)?).map_err(|_| TokenError::Malformed)?;
        if claims.exp() <= now {
            return Err(TokenError::Expired);
        }
        Ok(claims)
    }
}

pub trait HasExpiry {
    fn exp(&self) -> i64;
}

// ---------------------------------------------------------------------------------------------

/// Time-based one-time passwords for staff sign-in (RFC 6238: HMAC-SHA-1, 6 digits, 30 s),
/// compatible with every authenticator app.
pub mod totp {
    use hmac::{Hmac, KeyInit, Mac};
    use sha1::Sha1;

    pub const STEP_S: i64 = 30;

    /// The code for a time step (`unix_time / 30`).
    pub fn code(secret: &[u8], step: i64) -> String {
        let mut mac = Hmac::<Sha1>::new_from_slice(secret).expect("HMAC accepts any key length");
        mac.update(&step.to_be_bytes());
        let h = mac.finalize().into_bytes();
        let offset = (h[19] & 0x0f) as usize;
        let n = u32::from_be_bytes([h[offset] & 0x7f, h[offset + 1], h[offset + 2], h[offset + 3]]);
        format!("{:06}", n % 1_000_000)
    }

    /// The step `code` belongs to, allowing one step of clock skew either way, or `None`.
    /// Callers reject steps at or before the last one used, so a code works only once.
    pub fn verify(secret: &[u8], code_given: &str, unix_time: i64) -> Option<i64> {
        let now = unix_time / STEP_S;
        (now - 1..=now + 1).find(|&step| super::ct_eq(code(secret, step).as_bytes(), code_given.as_bytes()))
    }

    /// RFC 4648 base32 without padding, as authenticator apps expect.
    pub fn base32(bytes: &[u8]) -> String {
        const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
        let mut out = String::new();
        let (mut buf, mut bits) = (0u32, 0);
        for &b in bytes {
            buf = (buf << 8) | u32::from(b);
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                out.push(ALPHABET[((buf >> bits) & 31) as usize] as char);
            }
        }
        if bits > 0 {
            out.push(ALPHABET[((buf << (5 - bits)) & 31) as usize] as char);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret(s: &str) -> SecretString {
        SecretString::from(s.to_owned())
    }

    #[test]
    fn hashes_are_keyed_and_domain_separated() {
        let a = Hasher::new(&secret("k1-0123456789abcdef0123456789abcdef"));
        let b = Hasher::new(&secret("k2-0123456789abcdef0123456789abcdef"));
        assert_eq!(a.hash("phone", b"+264811234567"), a.hash("phone", b"+264811234567"));
        assert_ne!(a.hash("phone", b"+264811234567"), b.hash("phone", b"+264811234567"));
        assert_ne!(a.hash("phone", b"x"), a.hash("otp", b"x"));
    }

    #[test]
    fn seal_round_trips_and_is_bound_to_its_column() {
        let s = Sealer::new(&secret("seal-0123456789abcdef0123456789abcdef"));
        let sealed = s.seal_str("users.date_of_birth", "1994-03-12");
        assert_eq!(s.open_str("users.date_of_birth", &sealed).unwrap(), "1994-03-12");
        assert!(s.open_str("users.national_id", &sealed).is_err(), "moved to another column");
        let mut tampered = sealed.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(s.open("users.date_of_birth", &tampered).is_err());
        assert_ne!(sealed, s.seal_str("users.date_of_birth", "1994-03-12"), "nonce is random");
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Claims {
        sub: String,
        exp: i64,
    }
    impl HasExpiry for Claims {
        fn exp(&self) -> i64 {
            self.exp
        }
    }

    #[test]
    fn tokens_verify_and_reject_tampering() {
        let t = TokenSigner::new("k1", &secret("jwt-0123456789abcdef0123456789abcdef"), &[]);
        let token = t.sign(&Claims { sub: "u1".into(), exp: 1_000 });
        assert_eq!(t.verify::<Claims>(&token, 999).unwrap().sub, "u1");
        assert_eq!(t.verify::<Claims>(&token, 1_000), Err(TokenError::Expired));

        // Swap the payload for one claiming another user: the signature no longer matches.
        let parts: Vec<&str> = token.split('.').collect();
        let forged = format!("{}.{}.{}", parts[0], B64.encode(br#"{"sub":"u2","exp":1000}"#), parts[2]);
        assert_eq!(t.verify::<Claims>(&forged, 0), Err(TokenError::BadSignature));

        // alg:none is refused.
        let none = format!("{}.{}.", B64.encode(br#"{"alg":"none","typ":"JWT","kid":"k1"}"#), parts[1]);
        assert!(t.verify::<Claims>(&none, 0).is_err());

        // A token from another key is refused.
        let other = TokenSigner::new("k1", &secret("other-0123456789abcdef0123456789abcdef"), &[]);
        assert_eq!(other.verify::<Claims>(&token, 0), Err(TokenError::BadSignature));
    }

    #[test]
    fn totp_matches_rfc6238_vectors() {
        // RFC 6238 Appendix B, SHA-1 secret "12345678901234567890" (8-digit codes there;
        // the last 6 digits are the 6-digit code).
        let secret = b"12345678901234567890";
        for (t, expected) in [(59, "287082"), (1_111_111_109, "081804"), (1_234_567_890, "005924"), (2_000_000_000, "279037")] {
            assert_eq!(totp::code(secret, t / 30), expected, "t={t}");
        }
        assert_eq!(totp::verify(secret, "287082", 59), Some(1));
        assert_eq!(totp::verify(secret, "287082", 59 + 30), Some(1), "one step of skew");
        assert_eq!(totp::verify(secret, "287082", 59 + 90), None);
        assert_eq!(totp::base32(b"foobar"), "MZXW6YTBOI");
    }

    #[test]
    fn previous_keys_verify_during_rotation() {
        let old = TokenSigner::new("k1", &secret("old-0123456789abcdef0123456789abcdef"), &[]);
        let token = old.sign(&Claims { sub: "u1".into(), exp: 10 });
        let new = TokenSigner::new(
            "k2",
            &secret("new-0123456789abcdef0123456789abcdef"),
            &[("k1".into(), secret("old-0123456789abcdef0123456789abcdef"))],
        );
        assert!(new.verify::<Claims>(&token, 0).is_ok());
    }
}
