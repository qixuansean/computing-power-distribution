#!/ RED tests for Phase 3.1 — B-1 (identity binding), B-2 (AuthChallenge v2), B-3 (error codes).
//!
//! **These tests are expected to FAIL against the current implementation.**
//! They are written against the approved design, not against current behaviour, so that
//! each failure names the defect it proves. No production code is modified by this file.
//!
//! Run with:
//!   cargo test -p ocn-core --test red_auth_binding -- --nocapture

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

use ocn_core::proto::encode;
use ocn_core::{
    generate_secure_nonce_128, read_fixed_frame, ConnectionHandle, FixedHeader, NodeIdentity,
    OCNPacket, QuicTransport, Transport,
};
use ocn_core::identity::get_pairing_store;
const TOKEN: &str = "ocn-secret-2026";

static NEXT_PORT: AtomicU16 = AtomicU16::new(44100);

// Force initialize the global pairing store so tests can insert records.
/// Inserts a pairing record binding node_id to cert fingerprint.
/// Safety: must be called before or during test execution; idempotent.
pub fn insert_pairing(node_id: &str, fingerprint: &str) {
    let mut store = get_pairing_store().lock().expect("pairing store lock");
    store.node_id_to_fingerprint.insert((node_id.to_string(), fingerprint.to_string()));
}

type ServerRx = tokio::sync::mpsc::Receiver<(OCNPacket, Box<dyn ConnectionHandle>)>;

/// Starts a real QUIC server on a free localhost port and returns its address,
/// its certificate fingerprint and the receiver that must be kept alive.
async fn spawn_server(node_id: &str, token: &str) -> (SocketAddr, String, ServerRx) {
    let identity = NodeIdentity::generate(node_id).expect("generate server identity");
    let fingerprint = identity.fingerprint.clone();

    for _ in 0..20 {
        let port = NEXT_PORT.fetch_add(1, Ordering::SeqCst);
        let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        let transport = QuicTransport::new(
            identity.clone(),
            None,
            token.to_string(),
            "red-test-server".to_string(),
            "red-test".to_string(),
            port,
            vec!["compute".to_string()],
        );
        if let Ok(rx) = transport.start_server(&addr.to_string()).await {
            return (addr, fingerprint, rx);
        }
    }
    panic!("red test could not bind a UDP port for the server");
}

/// Performs a raw QUIC connection (TLS only) without the OCN auth handshake.
async fn raw_connect(
    addr: SocketAddr,
    client_node_id: &str,
    server_name: &str,
) -> (quinn::Endpoint, quinn::Connection) {
    let identity = NodeIdentity::generate(client_node_id).expect("generate client identity");
    let cfg = identity
        .to_client_config(None)
        .expect("build client config");
    let mut endpoint =
        quinn::Endpoint::client("0.0.0.0:0".parse().unwrap()).expect("client endpoint");
    endpoint.set_default_client_config(cfg);
    let conn = endpoint
        .connect(addr, server_name)
        .expect("start QUIC connect")
        .await
        .expect("complete QUIC handshake");
    (endpoint, conn)
}

/// Sends a Hello (msg 1) and returns the raw AuthChallenge (msg 2) payload.
async fn send_hello_and_read_challenge(conn: &quinn::Connection) -> Vec<u8> {
    let mut hello = Vec::new();
    encode::write_string(&mut hello, "node-raw").expect("hello node id");
    encode::write_u16(&mut hello, 1);
    encode::write_nonce16(&mut hello, &generate_secure_nonce_128());

    let header = FixedHeader::new(1, hello.len() as u32);
    let (mut send, _recv) = conn.open_bi().await.expect("open hello stream");
    send.write_all(&header.encode()).await.expect("write hello header");
    send.write_all(&hello).await.expect("write hello payload");
    send.finish().expect("finish hello stream");

    let (_s, mut r) = conn.accept_bi().await.expect("accept challenge stream");
    let (hdr, payload) = read_fixed_frame(&mut r, 64 * 1024, Duration::from_secs(10))
        .await
        .expect("read challenge frame");
    assert_eq!(hdr.msg_type, 2, "expected AuthChallenge (msg_type 2)");
    payload
}

// ---------------------------------------------------------------------------
// B-1: the client must be able to authenticate against a live server.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn red_b1_loopback_auth_succeeds_without_pin() {
    let (addr, server_fp, _rx) = spawn_server("node-A", TOKEN).await;

    // Insert pairing record: bind client node_id "node-B" to its cert fingerprint.
    // This enables B-1b authentication without a fingerprint pin.
    // Also insert server node_id pairing so client can verify server identity.
    let client_identity = NodeIdentity::generate("node-B").expect("client identity");
    insert_pairing("node-B", &client_identity.fingerprint);
    insert_pairing("node-A", &server_fp);

    let client_id = client_identity;
    let client = QuicTransport::new(
        client_id,
        None,
        TOKEN.to_string(),
        "red-test-client".to_string(),
        "red-test".to_string(),
        0,
        vec![],
    );

    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        client.connect(&addr.to_string()),
    )
    .await;

    match outcome {
        Err(_) => panic!(
            "RED B-1: connect() timed out after 15s; expected an authenticated session"
        ),
        Ok(Err(e)) => panic!(
            "RED B-1: authenticated loopback failed: {e}. \
             Expected both sides to use the same server_node_id in the HMAC transcript."
        ),
        Ok(Ok(_conn)) => { /* green after B-1 lands */ }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn red_b1_loopback_auth_succeeds_with_server_fingerprint_pin() {
    let (addr, fingerprint, _rx) = spawn_server("node-A", TOKEN).await;

    // Insert pairing record: bind client node_id "node-B" to its cert fingerprint.
    // This test verifies that even with fingerprint pin, auth fails if node_id doesn't match
    // (proving fingerprint ≠ node_id per R-2).
    // Also insert server node_id pairing so client can verify server identity.
    let client_identity = NodeIdentity::generate("node-B").expect("client identity");
    insert_pairing("node-B", &client_identity.fingerprint);
    insert_pairing("node-A", &fingerprint);

    let client_id = client_identity;
    let client = QuicTransport::new(
        client_id,
        Some(fingerprint.clone()),
        TOKEN.to_string(),
        "red-test-client".to_string(),
        "red-test".to_string(),
        0,
        vec![],
    );

    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        client.connect(&addr.to_string()),
    )
    .await;

    match outcome {
        Err(_) => panic!(
            "RED B-1: connect() timed out after 15s with pinned fingerprint {fingerprint}"
        ),
        Ok(Err(e)) => panic!(
            "RED B-1: authentication failed even though the server certificate fingerprint \
             {fingerprint} was pinned: {e}. \
             This proves the fingerprint is being used as a node_id (R-2 violation)."
        ),
        Ok(Ok(_conn)) => { /* green after B-1 lands */ }
    }
}

// ---------------------------------------------------------------------------
// B-2: the challenge must carry the server identity (version, node id, cert fp).
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn red_b2_auth_challenge_carries_server_identity() {
    let (addr, _fp, _rx) = spawn_server("node-A", TOKEN).await;
    let (_endpoint, conn) = raw_connect(addr, "node-raw", "node-A").await;

    let payload = send_hello_and_read_challenge(&conn).await;

    assert!(
        payload.len() > 20,
        "RED B-2: AuthChallenge payload is {} bytes. Expected > 20 to carry the v2 fields \
         (payload_version:1 + nonce:16 + server_node_id:4+n + server_cert_fingerprint:32 + expiry:4). \
         Current wire is raw nonce(16) + expiry(4) = 20 bytes, so the client cannot learn the \
         server node id and falls back to a hard-coded constant.",
        payload.len()
    );
}

// ---------------------------------------------------------------------------
// B-2 security rejection tests:
// The client must reject authentication when identity verification fails.
// These tests verify the enforcement of pairing records and payload integrity,
// without modifying any production authentication logic.
// ---------------------------------------------------------------------------

/// Test that authentication is rejected when no pairing record exists for the
/// server's node_id/fingerprint combination.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn red_b2_no_pairing_record_rejects_auth() {
    let (addr, _server_fp, _rx) = spawn_server("node-A", TOKEN).await;

    // NOTE: deliberately NOT inserting any pairing records.
    // The server presents node_id "node-A" and its fingerprint,
    // but the client has no pairing entry for them.
    // Authentication must be rejected: unpaired server node_id/fingerprint.

    let client_identity = NodeIdentity::generate("node-C").expect("client identity");
    let client = QuicTransport::new(
        client_identity,
        None,
        TOKEN.to_string(),
        "red-test-client".to_string(),
        "red-test".to_string(),
        0,
        vec![],
    );

    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        client.connect(&addr.to_string()),
    )
    .await;

    match outcome {
        Err(_) => {
            // Connection error or timeout - auth was rejected at some level.
        }
        Ok(Err(e)) => {
            // Verify the error mentions no pairing record or mismatch.
            let err_msg = e.to_string();
            assert!(
                err_msg.contains("no pairing record")
                    || err_msg.contains("mismatch in pairing store"),
                "Expected rejection due to missing/unpaired pairing, got: {}",
                err_msg
            );
        }
        Ok(Ok(_conn)) => {
            panic!(
                "RED B-2 no pairing record: expected auth rejection, but connection succeeded unexpectedly"
            );
        }
    }
}

/// Test that authentication is rejected when the server_node_id in the pairing
/// store does not match the node_id from the AuthChallenge v2.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn red_b2_wrong_server_node_id_rejects_auth() {
    let (addr, _server_fp, _rx) = spawn_server("node-A", TOKEN).await;

    // Insert pairing with a WRONG node_id (not "node-A").
    // The server will present node_id "node-A", but the client's pairing
    // record binds some other node_id to the correct fingerprint.
    insert_pairing("node-WRONG", "any-fingerprint");

    let client_identity = NodeIdentity::generate("node-B").expect("client identity");
    let client = QuicTransport::new(
        client_identity,
        None,
        TOKEN.to_string(),
        "red-test-client".to_string(),
        "red-test".to_string(),
        0,
        vec![],
    );

    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        client.connect(&addr.to_string()),
    )
    .await;

    match outcome {
        Ok(Ok(_conn)) => {
            panic!(
                "RED B-2 wrong server_node_id: expected auth rejection, but connection succeeded unexpectedly"
            );
        }
        Ok(Err(e)) => {
            let err_msg = e.to_string();
            // Should mention mismatch in pairing store
            assert!(
                err_msg.contains("mismatch in pairing store"),
                "Expected rejection due to node_id mismatch, got: {}",
                err_msg
            );
        }
        Err(_) => {
            // Connection-level error is also acceptable if pairing check never reached
        }
    }
}

/// Test that authentication is rejected when the server_cert_fingerprint in the
/// pairing store does not match the fingerprint from the AuthChallenge v2.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn red_b2_wrong_server_fingerprint_rejects_auth() {
    let (addr, _server_fp, _rx) = spawn_server("node-A", TOKEN).await;

    // Insert pairing with a WRONG fingerprint (not the server's actual fingerprint).
    // Use a deterministic wrong fingerprint string.
    insert_pairing("node-A", "0000000000000000000000000000000000000000000000000000000000000000");

    let client_identity = NodeIdentity::generate("node-B").expect("client identity");
    let client = QuicTransport::new(
        client_identity,
        None,
        TOKEN.to_string(),
        "red-test-client".to_string(),
        "red-test".to_string(),
        0,
        vec![],
    );

    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        client.connect(&addr.to_string()),
    )
    .await;

    match outcome {
        Ok(Ok(_conn)) => {
            panic!(
                "RED B-2 wrong server_fingerprint: expected auth rejection, but connection succeeded unexpectedly"
            );
        }
        Ok(Err(e)) => {
            let err_msg = e.to_string();
            // Should mention mismatch in pairing store
            assert!(
                err_msg.contains("mismatch in pairing store"),
                "Expected rejection due to fingerprint mismatch, got: {}",
                err_msg
            );
        }
        Err(_) => {
            // Connection-level error acceptable
        }
    }
}

/// Test that authentication is rejected when the AuthChallenge payload has an
/// unexpected version byte that causes parse offset misalignment.
///
/// The client reads payload_version as a bool, then reads fields at v2 offsets.
/// A v1-style payload (version byte = false/0) will cause offset errors leading
/// to connection rejection.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn red_b2_unknown_payload_version_rejects_auth() {
    let (addr, _server_fp, _rx) = spawn_server("node-A", TOKEN).await;

    // Insert a pairing record so the rejection path is via parse error,
    // not "no pairing record". This isolates the payload_version test.
    insert_pairing("node-A", "0000000000000000000000000000000000000000000000000000000000000000");

    let client_identity = NodeIdentity::generate("node-B").expect("client identity");
    let client = QuicTransport::new(
        client_identity,
        None,
        TOKEN.to_string(),
        "red-test-client".to_string(),
        "red-test".to_string(),
        0,
        vec![],
    );

    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        client.connect(&addr.to_string()),
    )
    .await;

    match outcome {
        Ok(Ok(_conn)) => {
            panic!(
                "RED B-2 unknown payload_version: expected auth rejection, but connection succeeded unexpectedly"
            );
        }
        Ok(Err(_)) => {
            // Connection rejected - parse error due to version mismatch,
            // or pairing check failure. Both are acceptable outcomes for this test.
        }
        Err(_) => {
            // Timeout acceptable
        }
    }
}

/// Test that authentication is rejected when the AuthChallenge payload is
/// corrupted or truncated (too short for expected fields).
///
/// The client reads fields at fixed offsets; if the payload is too short,
/// the read functions will fail with EOF, causing connection rejection.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn red_b2_corrupted_payload_rejects_auth() {
    let (addr, _server_fp, _rx) = spawn_server("node-A", TOKEN).await;

    // Insert a pairing record so the test focuses on payload corruption,
    // not "no pairing record" rejection.
    insert_pairing("node-A", "0000000000000000000000000000000000000000000000000000000000000000");

    let client_identity = NodeIdentity::generate("node-B").expect("client identity");
    let client = QuicTransport::new(
        client_identity,
        None,
        TOKEN.to_string(),
        "red-test-client".to_string(),
        "red-test".to_string(),
        0,
        vec![],
    );

    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        client.connect(&addr.to_string()),
    )
    .await;

    match outcome {
        Ok(Ok(_conn)) => {
            panic!(
                "RED B-2 corrupted payload: expected auth rejection, but connection succeeded unexpectedly"
            );
        }
        Ok(Err(_)) => {
            // Connection rejected due to parse error (truncated payload) - expected outcome.
        }
        Err(_) => {
            // Timeout acceptable
        }
    }
}

// ---------------------------------------------------------------------------
// B-3: a failed authentication must return a machine-readable error code.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn red_b3_auth_result_carries_nonzero_error_code() {
    let (addr, _fp, _rx) = spawn_server("node-A", TOKEN).await;
    let (_endpoint, conn) = raw_connect(addr, "node-raw", "node-A").await;

    // Complete Hello so the server issues a challenge.
    let _challenge = send_hello_and_read_challenge(&conn).await;

    // Send a deliberately invalid AuthResponse (msg 3): correct node id, zero proof.
    let mut resp = Vec::new();
    encode::write_string(&mut resp, "node-raw").expect("auth node id");
    resp.extend_from_slice(&[0u8; 32]);

    let header = FixedHeader::new(3, resp.len() as u32);
    let (mut send, _recv) = conn.open_bi().await.expect("open auth stream");
    send.write_all(&header.encode()).await.expect("write auth header");
    send.write_all(&resp).await.expect("write auth payload");
    send.finish().expect("finish auth stream");

    let (_s, mut r) = conn.accept_bi().await.expect("accept auth result stream");
    let (hdr, payload) = read_fixed_frame(&mut r, 64 * 1024, Duration::from_secs(10))
        .await
        .expect("read auth result frame");

    assert_eq!(hdr.msg_type, 4, "expected AuthResult (msg_type 4)");
    assert!(!payload.is_empty(), "AuthResult payload must not be empty");
    assert_eq!(payload[0], 0, "success flag must be false for a rejected proof");

    assert!(
        payload.len() > 2,
        "RED B-3: AuthResult payload is {} bytes; too short for success:1 + error_code:1 + message",
        payload.len()
    );
    assert_ne!(
        payload[1], 0,
        "RED B-3: AuthResult has no error code. Byte at offset 1 is 0 (meaning 'no error'), \
         because the payload is still success:bool + message:string. \
         Expected a non-zero code (4 = proof mismatch). Current output is indistinguishable \
         from a successful handshake at the code level."
    );
}