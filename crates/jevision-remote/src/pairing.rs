//! Pairing codes, the phone's key and what it signs.

use std::net::SocketAddr;

use iroh::{EndpointId, RelayUrl};

use crate::{RemoteError, hex, unhex};

/// Length of a SEC1 uncompressed P-256 public key: `0x04`, then x and y.
#[cfg(feature = "host")]
pub(crate) const DEVICE_KEY_LEN: usize = 65;
pub(crate) const CODE_LEN: usize = 16;
pub(crate) const NONCE_LEN: usize = 32;

/// How a pairing offer is written: the phone's QR code and the assistant's link differ in both,
/// so one is never taken for the other.
struct Link {
    scheme: &'static str,
    version: &'static str,
}

const PHONE: Link = Link {
    scheme: "jevision://pair?",
    version: "1",
};
const ASSISTANT: Link = Link {
    scheme: "jevision://assistant?",
    version: "2",
};

/// The phone's own key. On iOS it lives in the Secure Enclave and cannot be copied out.
pub trait DeviceSigner: Send + Sync {
    /// The P-256 public key in SEC1 uncompressed form: 65 bytes starting with `0x04`.
    fn public_key(&self) -> Vec<u8>;
    /// An ECDSA P-256 signature over the SHA-256 digest of `message`, DER-encoded: what
    /// `SecKeyCreateSignature` returns for `ecdsaSignatureMessageX962SHA256`.
    fn sign(&self, message: Vec<u8>) -> Result<Vec<u8>, RemoteError>;
}

/// What the Mac's pairing QR code holds. The code works once, for five minutes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingOffer {
    /// The Mac's endpoint id, hex.
    pub host_id: String,
    /// The Mac's name as its owner sees it.
    pub host_name: String,
    /// The one-time code, 32 hex digits.
    pub code: String,
    /// When the code stops working, in seconds since 1970. The Mac checks it; the phone may use
    /// it to say so before trying.
    pub expires_at: i64,
    /// The Mac's home relay, if it has one.
    pub relay_url: Option<String>,
    /// The Mac's direct addresses, such as `192.168.1.20:52114`.
    pub addrs: Vec<String>,
}

impl PairingOffer {
    /// The text of the QR code: `jevision://pair?v=1&id=…&name=…&code=…&exp=…&relay=…&addr=…`.
    pub fn to_url(&self) -> String {
        self.link(&PHONE)
    }

    /// The text an assistant is paired with: `jevision://assistant?v=2&id=…`, otherwise the
    /// same as [`PairingOffer::to_url`].
    pub fn to_assistant_url(&self) -> String {
        self.link(&ASSISTANT)
    }

    fn link(&self, link: &Link) -> String {
        let mut url = format!(
            "{}v={}&id={}&name={}&code={}&exp={}",
            link.scheme,
            link.version,
            self.host_id,
            encode(&self.host_name),
            self.code,
            self.expires_at
        );
        if let Some(relay) = &self.relay_url {
            url.push_str(&format!("&relay={}", encode(relay)));
        }
        for addr in &self.addrs {
            url.push_str(&format!("&addr={}", encode(addr)));
        }
        url
    }

    /// Reads a scanned QR code. Expiry is left to the Mac, whose clock decides.
    pub fn parse(text: &str) -> Result<Self, RemoteError> {
        Self::read(text, &PHONE)
    }

    /// Reads the link an assistant is paired with.
    pub fn parse_assistant(text: &str) -> Result<Self, RemoteError> {
        Self::read(text, &ASSISTANT)
    }

    fn read(text: &str, link: &Link) -> Result<Self, RemoteError> {
        let text = text.trim();
        let query = match text.get(..link.scheme.len()) {
            Some(head) if head.eq_ignore_ascii_case(link.scheme) => &text[link.scheme.len()..],
            _ => return Err(RemoteError::OfferWrongScheme),
        };
        let mut version = None;
        let mut host_id = None;
        let mut host_name = None;
        let mut code = None;
        let mut expires_at = None;
        let mut relay_url = None;
        let mut addrs = Vec::new();
        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (key, value) = pair.split_once('=').ok_or(RemoteError::OfferMalformed)?;
            let value = decode(value).ok_or(RemoteError::OfferMalformed)?;
            match key {
                "v" => version = Some(value),
                "id" => host_id = Some(value),
                "name" => host_name = Some(value),
                "code" => code = Some(value),
                "exp" => expires_at = value.parse::<i64>().ok(),
                "relay" => relay_url = Some(value),
                "addr" => addrs.push(value),
                _ => {}
            }
        }
        let offer = Self {
            host_id: host_id.ok_or(RemoteError::OfferMalformed)?,
            host_name: host_name
                .filter(|n| !n.trim().is_empty())
                .ok_or(RemoteError::OfferMalformed)?,
            code: code.ok_or(RemoteError::OfferMalformed)?,
            expires_at: expires_at.ok_or(RemoteError::OfferMalformed)?,
            relay_url,
            addrs,
        };
        let valid = version.as_deref() == Some(link.version)
            && offer.host_id.parse::<EndpointId>().is_ok()
            && code_bytes(&offer.code).is_some()
            && offer
                .relay_url
                .as_deref()
                .is_none_or(|r| r.parse::<RelayUrl>().is_ok())
            && offer.addrs.iter().all(|a| a.parse::<SocketAddr>().is_ok());
        if valid {
            Ok(offer)
        } else {
            Err(RemoteError::OfferMalformed)
        }
    }
}

pub(crate) fn code_bytes(code: &str) -> Option<[u8; CODE_LEN]> {
    unhex(code)?.try_into().ok()
}

/// What a phone signs to pair: it binds the code to both endpoints and to the key being paired.
pub(crate) fn pair_transcript(
    code: &[u8; CODE_LEN],
    host: &EndpointId,
    phone: &EndpointId,
    device_key: &[u8],
) -> Vec<u8> {
    [
        b"jevision-pair-v1".as_slice(),
        code,
        host.as_bytes(),
        phone.as_bytes(),
        device_key,
    ]
    .concat()
}

/// What a paired phone signs to sign in on a connection between these two endpoints.
pub(crate) fn auth_transcript(
    nonce: &[u8; NONCE_LEN],
    host: &EndpointId,
    phone: &EndpointId,
) -> Vec<u8> {
    [
        b"jevision-auth-v1".as_slice(),
        nonce,
        host.as_bytes(),
        phone.as_bytes(),
    ]
    .concat()
}

/// Whether `signature` (DER) is the device key's signature of `message`. The key must be SEC1
/// uncompressed.
#[cfg(feature = "host")]
pub(crate) fn verify(device_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    use p256::ecdsa::signature::Verifier;
    use p256::ecdsa::{Signature, VerifyingKey};

    if device_key.len() != DEVICE_KEY_LEN || device_key[0] != 0x04 {
        return false;
    }
    let (Ok(key), Ok(signature)) = (
        VerifyingKey::from_sec1_bytes(device_key),
        Signature::from_der(signature),
    ) else {
        return false;
    };
    key.verify(message, &signature).is_ok()
}

/// A device's id: the first 16 bytes of the SHA-256 of its public key, hex. Pairing the same
/// phone again keeps its id.
#[cfg(feature = "host")]
pub(crate) fn device_id(device_key: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    hex(&Sha256::digest(device_key)[..16])
}

/// Percent-encodes everything but RFC 3986 unreserved characters.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push_str(&hex(&[b]).to_uppercase());
        }
    }
    out
}

fn decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            out.extend(unhex(text.get(i + 1..i + 3)?)?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}
