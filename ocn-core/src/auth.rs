use std::collections::HashMap;
use tokio::time::{Instant, Duration};
use ring::hmac;
use ring::hkdf;

/// 128-bit CSPRNG Nonce (16 bytes)
pub type Nonce16 = [u8; 16];

/// 256-bit TLS Exporter Secret / Session Binding (32 bytes)
pub type Binding32 = [u8; 32];

/// 256-bit HMAC-SHA256 Auth Proof (32 bytes)
pub type AuthProof32 = [u8; 32];

/// Pending Challenge entry held by server during authentication handshake
pub struct PendingChallenge {
    pub peer_node_id: String,
    pub challenge_nonce: Nonce16,
    pub tls_session_binding: Binding32,
    pub created_at: Instant,
    pub expires_at: Instant,
    pub consumed: bool,
}

/// Thread-safe Pending Challenge Store with global & per-peer capacity limits and atomic consumption
pub struct PendingChallengeStore {
    challenges: HashMap<Nonce16, PendingChallenge>,
    peer_counts: HashMap<String, usize>,
    ttl: Duration,
    global_max_capacity: usize,
    per_peer_max_capacity: usize,
}

impl PendingChallengeStore {
    pub fn new(ttl_secs: u64, global_max_capacity: usize, per_peer_max_capacity: usize) -> Self {
        Self {
            challenges: HashMap::new(),
            peer_counts: HashMap::new(),
            ttl: Duration::from_secs(ttl_secs),
            global_max_capacity,
            per_peer_max_capacity,
        }
    }

    /// Purge expired challenges based on local Instant monotonic clock
    fn purge_expired(&mut self, now: Instant) {
        let mut expired_nonces = Vec::new();
        for (nonce, challenge) in &self.challenges {
            if now >= challenge.expires_at || challenge.consumed {
                expired_nonces.push((*nonce, challenge.peer_node_id.clone()));
            }
        }
        for (nonce, peer_id) in expired_nonces {
            self.challenges.remove(&nonce);
            if let Some(count) = self.peer_counts.get_mut(&peer_id) {
                if *count > 0 {
                    *count -= 1;
                }
            }
        }
    }

    /// Create and store a new challenge nonce for a peer. Returns Error if capacity limits are exceeded.
    pub fn create_challenge(
        &mut self,
        peer_node_id: &str,
        challenge_nonce: Nonce16,
        tls_session_binding: Binding32,
    ) -> anyhow::Result<()> {
        let now = Instant::now();
        self.purge_expired(now);

        if self.challenges.len() >= self.global_max_capacity {
            anyhow::bail!("Global PendingChallengeStore capacity exceeded ({})", self.global_max_capacity);
        }

        let current_peer_count = self.peer_counts.get(peer_node_id).copied().unwrap_or(0);
        if current_peer_count >= self.per_peer_max_capacity {
            anyhow::bail!("Per-peer PendingChallengeStore capacity exceeded for peer '{}' ({})", peer_node_id, self.per_peer_max_capacity);
        }

        let challenge = PendingChallenge {
            peer_node_id: peer_node_id.to_string(),
            challenge_nonce,
            tls_session_binding,
            created_at: now,
            expires_at: now + self.ttl,
            consumed: false,
        };

        self.challenges.insert(challenge_nonce, challenge);
        *self.peer_counts.entry(peer_node_id.to_string()).or_insert(0) += 1;

        Ok(())
    }

    /// Atomically verify and consume a challenge response. Single-use only.
    pub fn check_and_consume(
        &mut self,
        challenge_nonce: &Nonce16,
        client_node_id: &str,
        tls_session_binding: &Binding32,
    ) -> anyhow::Result<()> {
        let now = Instant::now();
        self.purge_expired(now);

        let challenge = match self.challenges.get_mut(challenge_nonce) {
            Some(c) => c,
            None => anyhow::bail!("Challenge nonce not found in store"),
        };

        if challenge.consumed {
            anyhow::bail!("Challenge nonce has already been consumed");
        }

        if now >= challenge.expires_at {
            anyhow::bail!("Challenge nonce has expired");
        }

        if challenge.peer_node_id != client_node_id {
            anyhow::bail!("Challenge peer node ID mismatch: expected '{}', got '{}'", challenge.peer_node_id, client_node_id);
        }

        if &challenge.tls_session_binding != tls_session_binding {
            anyhow::bail!("TLS session exporter binding mismatch");
        }

        // Atomically mark consumed and remove
        challenge.consumed = true;
        let peer_id = challenge.peer_node_id.clone();
        self.challenges.remove(challenge_nonce);
        if let Some(count) = self.peer_counts.get_mut(&peer_id) {
            if *count > 0 {
                *count -= 1;
            }
        }

        Ok(())
    }
}

/// Replay Cache Key bound to Peer Node ID and Nonce
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct ReplayKey {
    pub peer_node_id: String,
    pub nonce: Nonce16,
}

/// Replay Cache Entry with local monotonic insertion timestamp
pub struct ReplayEntry {
    pub inserted_at: Instant,
}

/// Thread-safe Replay Cache with capacity limits and TTL eviction
pub struct ReplayCache {
    entries: HashMap<ReplayKey, ReplayEntry>,
    ttl: Duration,
    max_capacity: usize,
}

impl ReplayCache {
    pub fn new(ttl_secs: u64, max_capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            ttl: Duration::from_secs(ttl_secs),
            max_capacity,
        }
    }

    /// Atomically check if (peer_node_id, nonce) has been used, and insert if new.
    /// Never evicts active unexpired entries if capacity is reached.
    pub fn check_and_insert(&mut self, peer_node_id: &str, nonce: &Nonce16) -> anyhow::Result<()> {
        let now = Instant::now();

        // Auto purge expired entries
        let ttl = self.ttl;
        self.entries.retain(|_, entry| now.duration_since(entry.inserted_at) < ttl);

        let key = ReplayKey {
            peer_node_id: peer_node_id.to_string(),
            nonce: *nonce,
        };

        if self.entries.contains_key(&key) {
            anyhow::bail!("Replay attack detected for peer '{}' and nonce", peer_node_id);
        }

        if self.entries.len() >= self.max_capacity {
            anyhow::bail!("ReplayCache max capacity ({}) reached; rejecting new nonce", self.max_capacity);
        }

        self.entries.insert(key, ReplayEntry { inserted_at: now });
        Ok(())
    }
}

/// Quinn helper: extract TLS Exporter binding material (RFC 5705)
pub fn extract_tls_exporter_binding(connection: &quinn::Connection) -> anyhow::Result<Binding32> {
    let mut exporter_output = [0u8; 32];
    let label = b"EXPORTER-OCN-TLS13-SESSION-BINDING";
    let context = b"OCN-v1-auth";

    connection.export_keying_material(&mut exporter_output, label, context)
        .map_err(|e| anyhow::anyhow!("TLS exporter keying material extraction failed: {:?}", e))?;
    Ok(exporter_output)
}

/// HKDF-SHA256 struct implementing ring's KeyType for HKDF expansion
struct OcnHkdfKeyType(usize);

impl hkdf::KeyType for OcnHkdfKeyType {
    fn len(&self) -> usize {
        self.0
    }
}

/// Derive a session authentication key from PairingToken and TLS Exporter Binding using HKDF-SHA256.
/// Raw exporter secret material is NEVER logged or persisted.
pub fn derive_session_auth_key(pairing_token: &str, exporter_binding: &Binding32) -> Binding32 {
    let salt = hkdf::Salt::new(hkdf::HKDF_SHA256, exporter_binding);
    let prk = salt.extract(pairing_token.as_bytes());
    let info: &[&[u8]] = &[b"OCN-v1-SESSION-AUTH-KEY"];

    let okm = prk.expand(info, OcnHkdfKeyType(32)).expect("HKDF expansion failed");
    let mut session_key = [0u8; 32];
    okm.fill(&mut session_key).expect("HKDF fill failed");
    session_key
}

/// Compute HMAC-SHA256 AuthProof over length-prefixed transcript:
/// Transcript = "OCN-v1-AUTH-DOMAIN" (18 bytes) || Version (2 bytes BE) || ServerChallenge[16] || ClientNonce[16]
///              || Len(ClientNodeID) (u32 BE) || ClientNodeID || Len(ServerNodeID) (u32 BE) || ServerNodeID
pub fn compute_auth_proof(
    session_auth_key: &Binding32,
    server_challenge_nonce: &Nonce16,
    client_nonce: &Nonce16,
    client_node_id: &str,
    server_node_id: &str,
) -> AuthProof32 {
    let mut transcript = Vec::new();

    // 1. Domain tag
    transcript.extend_from_slice(b"OCN-v1-AUTH-DOMAIN");

    // 2. Protocol version (u16 BE)
    transcript.extend_from_slice(&1u16.to_be_bytes());

    // 3. Server challenge nonce (16 bytes)
    transcript.extend_from_slice(server_challenge_nonce);

    // 4. Client nonce (16 bytes)
    transcript.extend_from_slice(client_nonce);

    // 5. Length-prefixed Client Node ID (u32 BE + bytes)
    let client_id_bytes = client_node_id.as_bytes();
    let client_id_len = client_id_bytes.len() as u32;
    transcript.extend_from_slice(&client_id_len.to_be_bytes());
    transcript.extend_from_slice(client_id_bytes);

    // 6. Length-prefixed Server Node ID (u32 BE + bytes)
    let server_id_bytes = server_node_id.as_bytes();
    let server_id_len = server_id_bytes.len() as u32;
    transcript.extend_from_slice(&server_id_len.to_be_bytes());
    transcript.extend_from_slice(server_id_bytes);

    let s_key = hmac::Key::new(hmac::HMAC_SHA256, session_auth_key);
    let tag = hmac::sign(&s_key, &transcript);
    let mut proof = [0u8; 32];
    proof.copy_from_slice(tag.as_ref());
    proof
}
