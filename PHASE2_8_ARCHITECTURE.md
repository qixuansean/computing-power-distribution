# OCN Phase 2.8 — Authenticated Node Communication Architecture Specification

**Document Version**: 2.1  
**Status**: Approved Specification (Stage 2 Enabled)  
**Project Path**: `C:\Users\qixua\Desktop\OCN`  

---

## 1. Complete Network Frame Format

### 1.1 Fixed Network Header (12 Bytes)

Every frame transmitted over a QUIC stream starts with a mandatory, fixed-size 12-byte header encoded strictly in **Big-Endian (Network Byte Order)**.

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                     Magic Header ("OCN1")                     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|       Protocol Version (u16)  |       Message Type (u16)      |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                       Payload Size (u32)                      |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

#### Header Fields Definition
| Offset | Field | Type | Encoding | Valid Values / Constraints |
|---|---|---|---|---|
| `0..4` | `magic` | `[u8; 4]` | Raw Bytes | Must equal `[0x4F, 0x43, 0x4E, 0x31]` (`"OCN1"`). Any other value results in immediate stream rejection and socket termination. |
| `4..6` | `version` | `u16` | Big-Endian | Must equal `1` (`PROTOCOL_VERSION`). Unsupported versions are rejected. |
| `6..8` | `msg_type` | `u16` | Big-Endian | Integer enum discriminant (`1` to `13`). Unknown tags cause immediate rejection. |
| `8..12` | `payload_size` | `u32` | Big-Endian | Length of subsequent payload bytes. Must not exceed packet-type size limits (`64 KB` for Control, `2 MB` for Data). |

```rust
pub const PROTOCOL_MAGIC: [u8; 4] = [0x4F, 0x43, 0x4E, 0x31]; // "OCN1"
pub const PROTOCOL_VERSION: u16 = 1;
pub const HEADER_SIZE: usize = 12;

pub const CONTROL_MAX_PAYLOAD_SIZE: u32 = 64 * 1024;      // 64 KB limit
pub const DATA_MAX_PAYLOAD_SIZE: u32 = 2 * 1024 * 1024;   // 2 MB limit

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixedHeader {
    pub magic: [u8; 4],
    pub version: u16,
    pub msg_type: u16,
    pub payload_size: u32,
}
```

---

## 2. Complete Message Payload Encoding

All message payloads are serialized into raw binary using explicit, fixed Big-Endian field-by-field layout. Field types obey the following strict rules:

- **`u16` / `u32` / `u64`**: Encoded in 2, 4, or 8 bytes Big-Endian (`to_be_bytes()`).
- **`bool`**: Encoded as a single `u8`. `0x01` represents `true`, `0x00` represents `false`. Any value `> 0x01` is rejected as illegal protocol state.
- **`[u8; 16]`**: Fixed 16 raw bytes (128-bit CSPRNG Nonces).
- **`String`**: Encoded as `len: u32` (Big-Endian) followed by `len` bytes of UTF-8 data. Max string length bounded to 16,384 bytes. If UTF-8 validation fails or `len` exceeds bounds, packet is rejected.
- **`Vec<u8>`**: Encoded as `len: u32` (Big-Endian) followed by `len` raw bytes. Max vector length bounded by message size limit.
- **`Vec<String>`**: Encoded as `count: u32` (Big-Endian) followed by `count` serialized `String` items. Max count bounded to 256.
- **`usize` / `isize`**: **PROHIBITED** in wire protocol definitions to eliminate architecture-dependent field size discrepancies between 32-bit/64-bit systems.

### 2.1 Message Type Registry & Detailed Layouts

| MsgType Tag | Name | Classification | Max Payload Size |
|---|---|---|---|
| `1` | `Hello` | Control | 64 KB |
| `2` | `AuthChallenge` | Control | 64 KB |
| `3` | `AuthResponse` | Control | 64 KB |
| `4` | `AuthResult` | Control | 64 KB |
| `5` | `NodeInfo` | Control | 64 KB |
| `6` | `Ping` | Control | 64 KB |
| `7` | `Pong` | Control | 64 KB |
| `8` | `Echo` | Control | 64 KB |
| `9` | `HashTest` | Data | 2 MB |
| `10` | `HashTestResult` | Control | 64 KB |
| `11` | `ThroughputChunk` | Data | 2 MB |
| `12` | `ThroughputAck` | Control | 64 KB |
| `13` | `Disconnect` | Control | 64 KB |

#### 1. `Hello` (Tag = 1)
- `client_node_id`: `String` (`u32` length + UTF-8 bytes)
- `protocol_version`: `u16` (Big-Endian)
- `client_nonce`: `[u8; 16]` (128-bit CSPRNG Nonce)

#### 2. `AuthChallenge` (Tag = 2)
- `server_challenge_nonce`: `[u8; 16]` (128-bit CSPRNG Nonce)
- `expires_in_secs`: `u32` (Big-Endian, e.g., 30s)

#### 3. `AuthResponse` (Tag = 3)
- `client_node_id`: `String`
- `auth_proof`: `[u8; 32]` (256-bit HMAC-SHA256 signature)

#### 4. `AuthResult` (Tag = 4)
- `success`: `bool` (`u8`: `0x01` or `0x00`)
- `message`: `String`

#### 5. `NodeInfo` (Tag = 5)
- `node_id`: `String`
- `platform_id`: `String`
- `listen_port`: `u16` (Big-Endian)
- `capabilities`: `Vec<String>` (`count: u32` + elements)

#### 6. `Ping` (Tag = 6)
- `timestamp_ms`: `u64` (Big-Endian)

#### 7. `Pong` (Tag = 7)
- `timestamp_ms`: `u64` (Big-Endian)

#### 8. `Echo` (Tag = 8)
- `payload`: `String`

#### 9. `HashTest` (Tag = 9)
- `data`: `Vec<u8>` (`u32` length + bytes)
- `expected_hash`: `String` (64-char Hex SHA-256)

#### 10. `HashTestResult` (Tag = 10)
- `success`: `bool` (`u8`: `0x01` or `0x00`)
- `computed_hash`: `String`

#### 11. `ThroughputChunk` (Tag = 11)
- `chunk_id`: `u32` (Big-Endian)
- `data`: `Vec<u8>`

#### 12. `ThroughputAck` (Tag = 12)
- `chunk_id`: `u32` (Big-Endian)

#### 13. `Disconnect` (Tag = 13)
- `reason`: `String`

### 2.2 Golden Test Vectors (Network Byte Validation)

To guarantee exact cross-platform byte alignment between x86_64 (Windows) and ARM64 (Android), unit tests validate serialized outputs against binary Golden Test Vectors.

#### Example: `Ping` Frame Golden Vector
- **Header**: Magic `"OCN1"` (`4F 43 4E 31`), Version `1` (`00 01`), MsgType `Ping` (`00 06`), PayloadSize `8` (`00 00 00 08`).
- **Payload**: Timestamp `1700000000000` (`00 00 01 8B 91 3C 24 00`).
- **Full Frame Hex String**:
  `4F434E3100010006000000080000018B913C2400` (20 bytes total).

---

## 3. TLS Session Binding & HMAC Transcript

To eliminate relay attacks and bind authentication proof strictly to the underlying active TLS 1.3 session, OCN extracts TLS keying material and derives a session authentication key using HKDF-SHA256.

### 3.1 TLS Exporter Keying Material Extraction
Using Quinn's official API `quinn::Connection::export_keying_material`:

```rust
pub fn extract_tls_exporter_binding(connection: &quinn::Connection) -> anyhow::Result<[u8; 32]> {
    let mut exporter_output = [0u8; 32];
    let label = b"EXPORTER-OCN-TLS13-SESSION-BINDING";
    let context = b"OCN-v1-auth";
    
    connection.export_keying_material(&mut exporter_output, label, context)?;
    Ok(exporter_output)
}
```

### 3.2 HKDF Key Derivation & HMAC Transcript Construction

Raw TLS Exporter secrets are **NEVER** stored, written to disk, or printed in logs. They exist only in ephemeral memory during proof calculation.

1. **HKDF-SHA256 Session Key Derivation**:
   $$\text{SessionAuthKey} = \text{HKDF-SHA256}\Big(\text{IKM} = \text{PairingToken}, \; \text{Salt} = \text{TLS-Exporter-Secret}\;[32], \; \text{Info} = \text{"OCN-v1-SESSION-AUTH-KEY"}\Big) \quad [32\text{ bytes}]$$

2. **Transcript Field Ordering (Length-Prefixed Node IDs)**:
   All string identifiers in the HMAC transcript are explicitly length-prefixed with a `u32` Big-Endian integer to eliminate ambiguity:

   $$\text{HMAC-Transcript} = \text{DomainTag}\;[18\text{ bytes}] \parallel \text{Version}\;[2\text{ bytes}] \parallel \text{ServerChallengeNonce}\;[16] \parallel \text{ClientNonce}\;[16] \parallel \text{Len}(\text{ClientNodeID})\;[4] \parallel \text{ClientNodeID} \parallel \text{Len}(\text{ServerNodeID})\;[4] \parallel \text{ServerNodeID}$$

   Where `DomainTag` = `b"OCN-v1-AUTH-DOMAIN"` (18 bytes) and `Version` = `1u16.to_be_bytes()` (2 bytes).

3. **HMAC-SHA256 AuthProof Computation**:
   $$\text{AuthProof} = \text{HMAC-SHA256}\Big(\text{SessionAuthKey}, \; \text{HMAC-Transcript}\Big) \quad [32\text{ bytes}]$$

---

## 4. Pending Challenge Lifecycle & Replay Protection

The server maintains a thread-safe `PendingChallengeStore` to manage challenges for active authentication requests.

```rust
use std::collections::HashMap;
use tokio::time::{Instant, Duration};

pub struct PendingChallenge {
    pub peer_node_id: String,
    pub challenge_nonce: [u8; 16],
    pub tls_session_binding: [u8; 32],
    pub created_at: Instant,
    pub expires_at: Instant,
    pub consumed: bool,
}

pub struct PendingChallengeStore {
    challenges: HashMap<[u8; 16], PendingChallenge>,
    peer_counts: HashMap<String, usize>, // Tracks per-peer active challenge count
    ttl: Duration,                       // Default: 30 seconds
    global_max_capacity: usize,         // Default: 10,000 entries
    per_peer_max_capacity: usize,       // Default: 10 entries per Peer
}
```

### 4.1 PendingChallengeStore Rules
- **Capacity Enforcement**:
  - Global Max Capacity: 10,000 active challenges.
  - Per-Peer Max Capacity: 10 active challenges per `peer_node_id`.
  - When limits are reached, the server **rejects new challenge requests immediately** and does NOT evict valid unexpired challenges.
- **Expiration & Purging**:
  - Outdated entries (`Instant::now() >= expires_at`) are automatically purged before checking capacity or verifying response.
- **Atomic Verification & Single-Use Consumption**:
  - Under a single write lock, the server verifies:
    1. Challenge exists for `challenge_nonce`.
    2. `peer_node_id` matches `client_node_id`.
    3. `tls_session_binding` matches connection exporter secret.
    4. Current time `< expires_at`.
    5. `consumed == false`.
  - Upon successful verification, `consumed` is atomically set to `true`, the entry is purged, and `peer_counts` is decremented.
- **Failure Handling & Rate Limiting**:
  - On any verification failure (invalid proof, expired challenge, replay), the server **immediately closes the QUIC connection** and applies a 1.0-second penalty delay on repeated auth attempts from that peer.

---

## 5. Strict TLS Certificate & Signature Verification

To prevent fail-open vulnerabilities, all TLS verifiers mandate strict expected certificate fingerprints. Unpaired nodes are rejected during TLS 1.3 handshake before any application-layer packet is processed.

### 5.1 Client-Side Verifier (`StrictServerCertVerifier`)
Verifies server certificate fingerprint and TLS 1.3 handshake signature:

```rust
pub struct StrictServerCertVerifier {
    pub expected_server_fingerprint: String, // MANDATORY: Fail-closed if mismatch
}

impl rustls::client::danger::ServerCertVerifier for StrictServerCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
        ctx.update(end_entity.as_ref());
        let cert_fp = hex::encode(ctx.finish().as_ref());

        if cert_fp != self.expected_server_fingerprint {
            return Err(rustls::Error::General("Server certificate fingerprint mismatch".into()));
        }
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::RSA_PSS_SHA256,
        ]
    }
}
```

### 5.2 Server-Side Verifier (`StrictClientCertVerifier`)
Mandates client certificate presentation and validates client certificate fingerprint and TLS 1.3 signature:

```rust
pub struct StrictClientCertVerifier {
    pub allowed_client_fingerprints: std::collections::HashSet<String>, // MANDATORY
}

impl rustls::server::danger::ClientCertVerifier for StrictClientCertVerifier {
    fn verify_client_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::server::danger::ClientCertVerified, rustls::Error> {
        let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
        ctx.update(end_entity.as_ref());
        let cert_fp = hex::encode(ctx.finish().as_ref());

        if !self.allowed_client_fingerprints.contains(&cert_fp) {
            return Err(rustls::Error::General("Client certificate fingerprint not paired".into()));
        }
        Ok(rustls::server::danger::ClientCertVerified::assertion())
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::RSA_PSS_SHA256,
        ]
    }

    fn root_hint_subjects(&self) -> &[rustls::DistinguishedName] {
        &[]
    }
}
```

---

## 6. Pairing & Key Storage (Platform Secure Storage)

No secrets or pairing data are written in plaintext to generic text or JSON files.

### 6.1 Windows Storage (DPAPI)
- **Path**: `%LOCALAPPDATA%\OpenComputeNetwork\paired_store.enc`
- **Protection**: Win32 DPAPI (`CryptProtectData` / `CryptUnprotectData`).
- **Access Scope**: `CRYPTPROTECT_UI_FORBIDDEN`, bound to current Windows User SID.

### 6.2 Android Keystore & Hardware Security Level Audit
- **Storage**: Encrypted Private Preferences / File via `AndroidKeyStore`.
- **Security Level Classification**:
  1. `SECURITY_LEVEL_STRONGBOX`: Dedicated hardware security chip (StrongBox Keymaster). Log `SECURITY_LEVEL_STRONGBOX`.
  2. `SECURITY_LEVEL_TRUSTED_ENVIRONMENT`: Processor TEE / TrustZone. Log `SECURITY_LEVEL_TEE`.
  3. `SECURITY_LEVEL_SOFTWARE`: Software-backed fallback. Log `SECURITY_LEVEL_SOFTWARE_WARNING`. **Software fallback MUST NOT claim hardware TEE security in reports or diagnostics.**
- **Plaintext Prohibition**: Plaintext key dumping to generic files is strictly forbidden.
- **Revocation**: API `revoke_pairing(peer_node_id)` instantly wipes keys from encrypted store.
- **Log Redaction**: All tokens, challenges, and proofs are replaced with `[REDACTED_SECRET]` in logs.

---

## 7. Memory & Resource Limits

To prevent memory exhaustion and Denial-of-Service attacks, the node enforces strict memory budgets and queue bounds:

| Parameter | Limit | Description |
|---|---|---|
| **Per-Connection Memory Budget** | `8 MB` | Max memory buffer allocated per active QUIC connection |
| **Global Memory Budget** | `64 MB` | Total memory budget for all active network connections |
| **Max Pending Messages** | `100` messages | Receiver channel depth per connection (`mpsc::channel(100)`) |
| **Max String Length** | `16,384` bytes | Max allowed bytes for serialized `String` fields |
| **Max Vector Count** | `256` elements | Max allowed count for `Vec<String>` fields |
| **Auth Request Rate Limiter** | `5` requests / sec | Per-peer authentication rate limit before penalty throttling |
| **Total Frame Deadline** | `10.0` seconds | Absolute single deadline for complete frame (Header + Payload) read |

---

## 8. Test Cases Inventory

### 8.1 Unit & Serialization Tests (`ocn-core`)
1. `test_fixed_header_golden_vectors`: Exact hex byte validation of 12-byte headers across all message types.
2. `test_field_encoding_primitives`: Validate `bool` (`0x01`/`0x00`), `u32`, `String`, `[u8; 16]` Big-Endian serialization.
3. `test_unknown_tag_and_version_rejection`: Reject magic `!= "OCN1"`, version `!= 1`, or tag `> 13`.
4. `test_hkdf_hmac_auth_proof_calculation`: HKDF-SHA256 key derivation + HMAC transcript calculation with length-prefixed Node IDs.
5. `test_pending_challenge_atomic_lifecycle`: Validate challenge creation, expiration (30s), atomic consumption, and per-peer/global capacity enforcement.
6. `test_replay_cache_capacity_and_ttl`: Test atomic check-and-insert, local Instant TTL purge, and capacity rejection.
7. `test_strict_tls13_verifiers`: Test certificate fingerprint mismatch rejection and TLS 1.3 signature verification.
8. `test_frame_total_deadline_timeout`: Ensure slow-loris read stream exceeding total deadline times out.

### 8.2 Real Physical Device Validation Sequence
1. **Phase A (Android -> Windows)**: POCO M6 dynamically discovers Windows node via mDNS, connects over QUIC TLS 1.3, completes Challenge-Response HMAC authentication using derived HKDF session key, exchanges `Hello` / `NodeInfo` / `Ping` / `Pong`, and logs `OCN_AUTH_COMM_SUCCESS`.
2. **Phase B (Windows -> Android)**: Windows node connects to POCO M6 over QUIC TLS 1.3, authenticates, and exchanges messages.
3. **Phase C (Security Rejections)**: Verify unpaired certificate fingerprint or invalid auth proof is immediately rejected without falling back to unauthenticated connection.

---

*Specification Approved. Entering Phase 2.8 Stage 2 Implementation.*
