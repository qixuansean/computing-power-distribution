//! End-to-end MS-Compute-Loopback tests.
//!
//! These tests use a real QUIC transport on localhost: the B-2 authentication handshake
//! runs first, then compute tasks are exchanged over independent QUIC bidirectional
//! streams. The server executes each task through the real `ComputeExecutor`; the client
//! re-computes the expected SHA-256 independently and verifies the returned result hash.
//!
//! Covered scenarios:
//! - successful batch-hash execution (real executor, real transport)
//! - invalid input (input hash mismatch and unsupported wire version)
//! - duplicate task id replay from the deduplication cache
//! - client-side timeout while the server is unresponsive
//! - interrupted stream followed by a successful task on the same connection
//! - concurrent tasks, one stream each

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ocn_compute::server::{TaskClient, TaskServer};
use ocn_core::identity::get_pairing_store;
use ocn_core::task_wire::{error_code, read_task_frame, write_task_frame};
use ocn_core::{
    ConnectionHandle, ConnectionState, NodeIdentity, OCNPacket, QuicConnection, QuicTransport,
    TaskRequestFrame, TaskResultFrame, Transport,
};
use sha2::{Digest, Sha256};

const TOKEN: &str = "ocn-secret-2026";
static NEXT_PORT: AtomicU16 = AtomicU16::new(46100);
static NEXT_NODE: AtomicU16 = AtomicU16::new(0);

type ServerRx = tokio::sync::mpsc::Receiver<(OCNPacket, Box<dyn ConnectionHandle>)>;

fn unique_node(prefix: &str) -> String {
    let id = NEXT_NODE.fetch_add(1, Ordering::SeqCst);
    format!("{prefix}-{id}")
}

fn insert_pairing(node_id: &str, fingerprint: &str) {
    let mut store = get_pairing_store().lock().expect("pairing store lock");
    store
        .node_id_to_fingerprint
        .insert((node_id.to_string(), fingerprint.to_string()));
}

/// Start a real QUIC server on an available localhost port.
async fn start_server(node_id: &str) -> (SocketAddr, String, ServerRx) {
    let identity = NodeIdentity::generate(node_id).expect("server identity");
    let fingerprint = identity.fingerprint.clone();

    for _ in 0..50 {
        let port = NEXT_PORT.fetch_add(1, Ordering::SeqCst);
        let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        let transport = QuicTransport::new(
            identity.clone(),
            None,
            TOKEN.to_string(),
            format!("server-{node_id}"),
            "windows".to_string(),
            port,
            vec!["compute".to_string()],
        );
        if let Ok(rx) = transport.start_server(&addr.to_string()).await {
            return (addr, fingerprint, rx);
        }
    }
    panic!("could not bind a UDP port for the compute server");
}

/// Start a server plus a task-serving loop that runs the real ComputeExecutor.
async fn spawn_compute_server(node_id: &str) -> (SocketAddr, String, Arc<TaskServer>) {
    let (addr, fingerprint, mut rx) = start_server(node_id).await;
    let server = Arc::new(TaskServer::new(node_id));

    let accept_server = server.clone();
    tokio::spawn(async move {
        while let Some((_packet, handle)) = rx.recv().await {
            let server = accept_server.clone();
            tokio::spawn(async move {
                if let Some(connection) = handle.as_quic() {
                    server.serve_connection(connection).await;
                }
            });
        }
    });

    (addr, fingerprint, server)
}

/// Perform the real B-2 authenticated client connection.
async fn connect_client(
    addr: SocketAddr,
    server_id: &str,
    server_fingerprint: &str,
    client_id: &str,
) -> Box<dyn ConnectionHandle> {
    let client_identity = NodeIdentity::generate(client_id).expect("client identity");
    insert_pairing(client_id, &client_identity.fingerprint);
    insert_pairing(server_id, server_fingerprint);

    let transport = QuicTransport::new(
        client_identity,
        Some(server_fingerprint.to_string()),
        TOKEN.to_string(),
        format!("client-{client_id}"),
        "windows".to_string(),
        0,
        vec![],
    );

    transport
        .connect(&addr.to_string())
        .await
        .expect("client connect and B-2 auth")
}

fn expected_sha256(payload: &[u8]) -> [u8; 32] {
    Sha256::digest(payload).into()
}

/// Cheap independently owned view of an authenticated QUIC connection.
///
/// `quinn::Connection` is a reference-counted handle, so this can be moved into
/// spawned tasks without keeping the original `ConnectionHandle` alive.
fn owned_connection(connection: &QuicConnection) -> QuicConnection {
    QuicConnection {
        connection: connection.connection.clone(),
        state: ConnectionState::Active,
        peer_node_id: connection.peer_node_id.clone(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quic_loopback_successful_execution() {
    let server_id = unique_node("srv-ok");
    let client_id = unique_node("cli-ok");
    let (addr, fingerprint, server) = spawn_compute_server(&server_id).await;
    let handle = connect_client(addr, &server_id, &fingerprint, &client_id).await;
    let connection = handle.as_quic().expect("quic connection");

    let payload = vec![0x5Au8; 256 * 1024];
    let request = TaskRequestFrame::new("task-ok-1", payload.clone(), 10_000);
    let result = TaskClient::new(connection)
        .execute(&request)
        .await
        .expect("task execution");

    assert!(result.success, "execution failed: {:?}", result.error_msg);
    assert_eq!(result.task_id, "task-ok-1");
    assert_eq!(result.node_id, server_id);
    assert!(!result.duplicate);
    assert_eq!(result.result, payload, "result must be produced from the payload");
    assert_eq!(
        result.result_hash,
        expected_sha256(&payload),
        "client-side independent hash must match the server result hash"
    );
    assert_eq!(server.execution_count(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quic_loopback_rejects_invalid_input_hash() {
    let server_id = unique_node("srv-bad-hash");
    let client_id = unique_node("cli-bad-hash");
    let (addr, fingerprint, _server) = spawn_compute_server(&server_id).await;
    let handle = connect_client(addr, &server_id, &fingerprint, &client_id).await;
    let connection = handle.as_quic().expect("quic connection");

    let mut request = TaskRequestFrame::new("task-bad-hash", b"payload".to_vec(), 5_000);
    request.input_hash = [0u8; 32];

    let result = TaskClient::new(connection)
        .execute(&request)
        .await
        .expect("server must answer with a structured error result");

    assert!(!result.success);
    assert_eq!(result.error_code, Some(error_code::INVALID_TASK));
    let message = result.error_msg.unwrap_or_default();
    assert!(
        message.to_lowercase().contains("hash"),
        "unexpected error message: {message}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quic_loopback_rejects_unknown_wire_version() {
    let server_id = unique_node("srv-bad-version");
    let client_id = unique_node("cli-bad-version");
    let (addr, fingerprint, _server) = spawn_compute_server(&server_id).await;
    let handle = connect_client(addr, &server_id, &fingerprint, &client_id).await;
    let connection = handle.as_quic().expect("quic connection");

    let mut request = TaskRequestFrame::new("task-bad-version", b"payload".to_vec(), 5_000);
    request.wire_version = 99;

    // Bypass `open_task`'s local validation to prove the server rejects the frame too.
    let (mut send, mut recv) = connection.connection.open_bi().await.expect("open stream");
    write_task_frame(&mut send, &request).await.expect("write frame");
    send.finish().expect("finish request");

    let result: TaskResultFrame = read_task_frame(&mut recv, Duration::from_secs(5))
        .await
        .expect("read error result");
    assert!(!result.success);
    assert_eq!(result.error_code, Some(error_code::BAD_VERSION));
    assert_eq!(result.task_id, "task-bad-version");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quic_loopback_duplicate_task_is_not_reexecuted() {
    let server_id = unique_node("srv-dup");
    let client_id = unique_node("cli-dup");
    let (addr, fingerprint, server) = spawn_compute_server(&server_id).await;
    let handle = connect_client(addr, &server_id, &fingerprint, &client_id).await;
    let connection = handle.as_quic().expect("quic connection");
    let client = TaskClient::new(connection);

    let payload = b"duplicate task payload".to_vec();
    let request = TaskRequestFrame::new("task-dup", payload.clone(), 10_000);

    let first = client.execute(&request).await.expect("first execution");
    let second = client.execute(&request).await.expect("duplicate replay");

    assert!(first.success);
    assert!(second.success);
    assert!(!first.duplicate);
    assert!(second.duplicate, "second response must be served from the dedup cache");
    assert_eq!(first.result_hash, second.result_hash);
    assert_eq!(first.result_hash, expected_sha256(&payload));
    assert_eq!(
        server.execution_count(),
        1,
        "duplicate task ids must not be executed twice"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quic_loopback_client_timeout_when_server_is_silent() {
    let server_id = unique_node("srv-timeout");
    let client_id = unique_node("cli-timeout");
    let (addr, fingerprint, mut rx) = start_server(&server_id).await;

    // Complete auth, then keep the connection alive without serving any task stream.
    tokio::spawn(async move {
        if let Some((_packet, handle)) = rx.recv().await {
            let _held = handle;
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });

    let handle = connect_client(addr, &server_id, &fingerprint, &client_id).await;
    let connection = handle.as_quic().expect("quic connection");

    let request = TaskRequestFrame::new("task-timeout", b"slow".to_vec(), 5_000);
    let start = Instant::now();
    let outcome = TaskClient::new(connection)
        .execute_with_timeout(&request, Duration::from_millis(300))
        .await;
    let elapsed = start.elapsed();

    let error = outcome.expect_err("client must time out while the server is silent");
    assert!(
        error.to_string().contains("timed out"),
        "unexpected timeout error: {error}"
    );
    assert!(
        elapsed >= Duration::from_millis(250) && elapsed < Duration::from_secs(3),
        "timeout fired after {elapsed:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quic_loopback_survives_interrupted_stream() {
    let server_id = unique_node("srv-reset");
    let client_id = unique_node("cli-reset");
    let (addr, fingerprint, server) = spawn_compute_server(&server_id).await;
    let handle = connect_client(addr, &server_id, &fingerprint, &client_id).await;
    let connection = handle.as_quic().expect("quic connection");

    // Open a task stream, announce a large frame, send a partial body, then reset it.
    {
        let (mut send, recv) = connection.connection.open_bi().await.expect("open stream");
        send.write_all(&9999u32.to_be_bytes())
            .await
            .expect("write frame length");
        send.write_all(b"partial")
            .await
            .expect("write partial frame");
        send.reset(0u32.into()).expect("reset interrupted stream");
        drop(recv);
    }

    tokio::time::sleep(Duration::from_millis(200)).await;

    // The same authenticated connection must still serve a valid task.
    let payload = b"after interruption".to_vec();
    let request = TaskRequestFrame::new("task-after-reset", payload.clone(), 10_000);
    let result = TaskClient::new(connection)
        .execute(&request)
        .await
        .expect("task after interrupted stream");

    assert!(result.success);
    assert_eq!(result.result_hash, expected_sha256(&payload));
    assert_eq!(server.execution_count(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quic_loopback_concurrent_tasks_use_independent_streams() {
    let server_id = unique_node("srv-concurrent");
    let client_id = unique_node("cli-concurrent");
    let (addr, fingerprint, server) = spawn_compute_server(&server_id).await;
    let handle = connect_client(addr, &server_id, &fingerprint, &client_id).await;
    let connection = handle.as_quic().expect("quic connection");

    let mut tasks = Vec::new();
    for index in 0..8u8 {
        let payload = vec![index; 32 * 1024];
        let request = TaskRequestFrame::new(format!("task-concurrent-{index}"), payload.clone(), 15_000);
        let owned = owned_connection(connection);
        tasks.push(tokio::spawn(async move {
            let result = TaskClient::new(&owned)
                .execute(&request)
                .await
                .expect("concurrent task");
            (payload, result)
        }));
    }

    for task in tasks {
        let (payload, result) = task.await.expect("join concurrent task");
        assert!(result.success);
        assert_eq!(result.result_hash, expected_sha256(&payload));
    }
    assert_eq!(server.execution_count(), 8);
}
