//! Who may drive Sessions from a phone: the Remote store (`remote.json`) of paired phones and the
//! pairing codes that admit a new one. A phone pairs once, by presenting the code shown in
//! Settings (as a QR code, or typed), and gets a token it sends on every connection. Tokens are
//! random and stored hashed, so the file gives an attacker nothing to replay. Pure: no I/O.

use crate::model::RemoteDevice;
use base64::Engine;
use rand::{Rng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The port the server listens on by default (127.0.0.1 only; Tailscale Serve reaches it).
pub const DEFAULT_PORT: u16 = 47611;
/// How long a pairing code stays valid.
pub const PAIRING_TTL_MS: u64 = 10 * 60 * 1000;
/// Wrong codes before the pairing is cancelled.
pub const MAX_ATTEMPTS: u32 = 5;
/// Pairing code alphabet: no 0/O, 1/I, so a typed code has no lookalikes. 32 symbols, 8 of them
/// is 40 bits; at 5 attempts per pairing, unguessable.
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const CODE_LEN: usize = 8;

/// `remote.json`: whether Remote is on, its port, and the paired phones.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Store {
    pub enabled: bool,
    pub port: u16,
    pub devices: Vec<Device>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    /// Hex SHA-256 of the token.
    pub token_hash: String,
    pub created_at: u64,
    pub last_seen_at: Option<u64>,
    pub login: Option<String>,
}

impl Store {
    pub fn port(&self) -> u16 {
        if self.port == 0 {
            DEFAULT_PORT
        } else {
            self.port
        }
    }

    pub fn public(&self) -> Vec<RemoteDevice> {
        self.devices
            .iter()
            .map(|d| RemoteDevice {
                id: d.id.clone(),
                name: d.name.clone(),
                created_at: d.created_at,
                last_seen_at: d.last_seen_at,
                login: d.login.clone(),
            })
            .collect()
    }

    /// Add a phone; returns the token it must keep (the only time it exists in the clear).
    pub fn add(&mut self, name: &str, login: Option<String>, now: u64) -> (String, RemoteDevice) {
        let token = new_token();
        let name = name.trim();
        let device = Device {
            id: new_id(),
            name: if name.is_empty() { "Phone".into() } else { name.chars().take(60).collect() },
            token_hash: hash_token(&token),
            created_at: now,
            last_seen_at: None,
            login,
        };
        self.devices.push(device);
        let public = self.public().pop().expect("just pushed");
        (token, public)
    }

    /// The id of the phone `token` belongs to, noting when it was seen; None for a stranger.
    pub fn verify(&mut self, token: &str, now: u64) -> Option<String> {
        let presented = hash_token(token);
        let device = self
            .devices
            .iter_mut()
            .find(|d| constant_time_eq(d.token_hash.as_bytes(), presented.as_bytes()))?;
        device.last_seen_at = Some(now);
        Some(device.id.clone())
    }

    /// True when a phone was removed.
    pub fn revoke(&mut self, id: &str) -> bool {
        let before = self.devices.len();
        self.devices.retain(|d| d.id != id);
        self.devices.len() != before
    }
}

/// A pairing in progress.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairingCode {
    pub code: String,
    /// Epoch ms.
    pub expires_at: u64,
    pub attempts: u32,
}

/// What presenting a code to a pairing came to.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    /// Wrong code; the pairing goes on.
    Wrong,
    /// Wrong code, and that was the last try: the pairing is cancelled.
    Locked,
    Expired,
}

impl PairingCode {
    pub fn new(now: u64) -> Self {
        Self {
            code: new_code(),
            expires_at: now + PAIRING_TTL_MS,
            attempts: 0,
        }
    }

    pub fn expired(&self, now: u64) -> bool {
        now >= self.expires_at
    }

    /// Check a presented code (typed codes may carry spaces, dashes and lowercase).
    pub fn present(&mut self, presented: &str, now: u64) -> Verdict {
        if self.expired(now) {
            return Verdict::Expired;
        }
        let normalised: String = presented
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_uppercase())
            .collect();
        if constant_time_eq(normalised.as_bytes(), self.code.as_bytes()) {
            return Verdict::Ok;
        }
        self.attempts += 1;
        if self.attempts >= MAX_ATTEMPTS {
            Verdict::Locked
        } else {
            Verdict::Wrong
        }
    }

    /// The code with a space in the middle, as shown to the user for typing.
    pub fn display(&self) -> String {
        let (a, b) = self.code.split_at(CODE_LEN / 2);
        format!("{a} {b}")
    }
}

/// 256 random bits, base64url without padding.
pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn new_code() -> String {
    let mut rng = rand::rng();
    (0..CODE_LEN)
        .map(|_| CODE_ALPHABET[rng.random_range(0..CODE_ALPHABET.len())] as char)
        .collect()
}

fn new_id() -> String {
    let mut bytes = [0u8; 8];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Equality that takes the same time whatever the first differing byte.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_paired_phone_verifies_by_token_and_a_stranger_does_not() {
        let mut store = Store::default();
        let (token, device) = store.add("  Ben's iPhone ", Some("ben@github".into()), 1000);
        assert_eq!(device.name, "Ben's iPhone");
        assert_eq!(device.login.as_deref(), Some("ben@github"));
        assert!(!store.devices[0].token_hash.contains(&token), "stored hashed");
        assert_eq!(store.verify(&token, 2000), Some(device.id.clone()));
        assert_eq!(store.devices[0].last_seen_at, Some(2000));
        assert_eq!(store.verify("nope", 3000), None);
        assert_eq!(store.verify(&token[..10], 3000), None);

        let (other, _) = store.add("", None, 4000);
        assert_eq!(store.devices[1].name, "Phone");
        assert_ne!(token, other);
        assert!(store.revoke(&device.id));
        assert!(!store.revoke(&device.id));
        assert_eq!(store.verify(&token, 5000), None, "revoked");
        assert!(store.verify(&other, 5000).is_some());
    }

    #[test]
    fn the_store_round_trips_and_defaults_the_port() {
        let mut store = Store::default();
        assert_eq!(store.port(), DEFAULT_PORT);
        store.port = 5000;
        store.enabled = true;
        store.add("p", None, 1);
        let json = serde_json::to_string(&store).unwrap();
        assert_eq!(serde_json::from_str::<Store>(&json).unwrap(), store);
        assert_eq!(serde_json::from_str::<Store>("{}").unwrap(), Store::default());
    }

    #[test]
    fn pairing_accepts_the_code_in_any_typing_and_locks_after_five_misses() {
        let mut p = PairingCode::new(0);
        assert_eq!(p.code.len(), CODE_LEN);
        assert!(p.code.bytes().all(|b| CODE_ALPHABET.contains(&b)));
        assert_eq!(p.display().replace(' ', ""), p.code);
        let typed = format!(" {}-{} ", p.code[..4].to_lowercase(), &p.code[4..]);
        assert_eq!(p.present(&typed, 1), Verdict::Ok);
        assert_eq!(p.attempts, 0);

        for _ in 0..MAX_ATTEMPTS - 1 {
            assert_eq!(p.present("WRONG", 1), Verdict::Wrong);
        }
        assert_eq!(p.present("WRONG", 1), Verdict::Locked);
        assert_eq!(p.present(&p.code.clone(), PAIRING_TTL_MS), Verdict::Expired);
        assert!(p.expired(PAIRING_TTL_MS));
        assert!(!p.expired(PAIRING_TTL_MS - 1));
    }

    #[test]
    fn tokens_and_codes_are_fresh_every_time() {
        assert_ne!(new_token(), new_token());
        assert_ne!(new_code(), new_code());
        assert_eq!(new_token().len(), 43);
        assert_eq!(hash_token("a").len(), 64);
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }
}
