//! Same-connection concurrency and single-flight deduplication tests.
//!
//! These exercise the bounded, spawn-per-stream `TaskServer`. A real QUIC transport and
//! the real B-2 handshake are used, exactly like `quic_loopback.rs`; the server runs the
//! real `ComputeExecutor` and the client re-verifies every result hash independently.
//!
//! Covered:
//! - a stalled stream must not delay a sibling stream on the same connection
//! - eight concurrent tasks are admitted together (no serialized staircase)
//! - a concurrent duplicate waits and receives the cached result (single-flight)
//! - a stalled stream must not consume a sibling's timeout budget
//! - connection interruption must not corrupt other connections
//! - permits are released when a stream is cancelled or when tasks complete
//! - the in-flight admission limit is actually enforced

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ocn_compute::server::{TaskClient, TaskServer};
use ocn_core::identity::get_pairing_store;
use ocn_core::{
    ConnectionHandle, ConnectionState, NodeIdentity, QuicConnection, QuicTransport,
    TaskRequestFrame, TaskResultFrame, Transport,
};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

const TOKEN: &str = "ocn-concurrency-2026";
static NEXT_PORT: AtomicU16 = AtomicU16::new(46300);
static NEXT_NODE: AtomicU16 = AtomicU16::new(0);

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

/// Start a real QUIC server plus a task-serving loop. `max_inflight` overrides the
/// admission limit; `None` keeps the host-derived default.
async fn spawn_compute_server(
    node_id: &str,
    max_inflight: Option<usize>,
) -> (SocketAddr, String, Arc<TaskServer>) {
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
        let Ok(mut rx) = transport.start_server(&addr.to_string()).await else {
            continue;
        };

        let mut server = TaskServer::new(node_id);
        if let Some(limit) = max_inflight {
            server = server.with_max_inflight(limit);
        }
        let server = Arc::new(server);

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

        return (addr, fingerprint, server);
    }
    panic!("could not bind a UDP port for the concurrency server");
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

fn owned_connection(connection: &QuicConnection) -> QuicConnection {
    QuicConnection {
        connection: connection.connection.clone(),
        state: ConnectionState::Active,
        peer_node_id: connection.peer_node_id.clone(),
    }
}

fn expected_sha256(payload: &[u8]) -> [u8; 32] {
    Sha256::digest(payload).into()
}

/// Open a stream, announce a larger frame than is written, and keep it open.
///
/// The server's `recv_request` waits for the rest of the body until it is reset or until
/// `request_timeout` expires, so this is a deterministic "stalled stream".
async fn open_stalled_stream(connection: &QuicConnection) -> quinn::SendStream {
    let (mut send, _recv) = connection
        .connection
        .open_bi()
        .await
        .expect("open stalled stream");
    send.write_all(&4096u32.to_be_bytes())
        .await
        .expect("write announced length");
    send.write_all(b"stall")
        .await
        .expect("write partial body");
    send.flush().await.expect("flush partial body");
    send
}

fn millis(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn latency_ratio(latencies: &[f64]) -> f64 {
    let min = latencies.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = latencies.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if min <= 0.0 {
        return f64::INFINITY;
    }
    max / min
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stalled_stream_does_not_block_sibling() {
    let server_id = unique_node("srv-hol");
    let client_id = unique_node("cli-hol");
    let (addr, fingerprint, server) = spawn_compute_server(&server_id, None).await;
    let handle = connect_client(addr, &server_id, &fingerprint, &client_id).await;
    let connection = handle.as_quic().expect("quic connection");

    let mut stalled = open_stalled_stream(connection).await;

    let payload = b"stalled sibling payload".to_vec();
    let request = TaskRequestFrame::new("hol-sibling", payload.clone(), 10_000);
    let start = Instant::now();
    let result = TaskClient::new(connection)
        .execute_with_timeout(&request, Duration::from_secs(3))
        .await
        .expect("sibling task must not be blocked by the stalled stream");
    let elapsed = millis(start);

    assert!(result.success, "{:?}", result.error_msg);
    assert_eq!(result.result_hash, expected_sha256(&payload));
    assert!(
        elapsed < 2_000.0,
        "sibling task took {elapsed:.1} ms while another stream was stalled; \
         per-connection head-of-line blocking is still present"
    );
    assert_eq!(server.execution_count(), 1);

    let _ = stalled.reset(0u32.into());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn eight_concurrent_tasks_scale() {
    let server_id = unique_node("srv-scale");
    let client_id = unique_node("cli-scale");
    let (addr, fingerprint, server) = spawn_compute_server(&server_id, Some(8)).await;
    let handle = connect_client(addr, &server_id, &fingerprint, &client_id).await;
    let connection = handle.as_quic().expect("quic connection");

    let mut tasks = Vec::new();
    for index in 0..8u8 {
        let payload = vec![index; 4 * 1024 * 1024];
        let request = TaskRequestFrame::new(
            format!("scale-{index}"),
            payload.clone(),
            20_000,
        );
        let owned = owned_connection(connection);
        tasks.push(tokio::spawn(async move {
            let start = Instant::now();
            let outcome = TaskClient::new(&owned)
                .execute_with_timeout(&request, Duration::from_secs(15))
                .await;
            (payload, millis(start), outcome)
        }));
    }

    let batch_start = Instant::now();
    let mut latencies = Vec::new();
    for task in tasks {
        let (payload, latency, outcome) = task.await.expect("join concurrent task");
        let result: TaskResultFrame = outcome.expect("concurrent task");
        assert!(result.success, "{:?}", result.error_msg);
        assert_eq!(result.result, payload);
        assert_eq!(result.result_hash, expected_sha256(&payload));
        assert!(!result.duplicate);
        latencies.push(latency);
    }
    let wall = millis(batch_start);

    // A serialized accept loop produces a staircase: each task waits for the previous one.
    // Concurrent admission makes every task finish within a narrow band instead.
    let ratio = latency_ratio(&latencies);
    assert!(
        ratio < 2.0,
        "latency spread {ratio:.2} (min {:.1} ms, max {:.1} ms) indicates serialized handling",
        latencies.iter().cloned().fold(f64::INFINITY, f64::min),
        latencies.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
    );
    assert!(
        server.peak_inflight() >= 2,
        "server never admitted more than one task at a time"
    );
    assert_eq!(server.execution_count(), 8);
    assert!(wall < 10_000.0, "batch took {wall:.1} ms");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn duplicate_id_concurrent_is_coalesced() {
    let server_id = unique_node("srv-dup-concurrent");
    let client_id = unique_node("cli-dup-concurrent");
    let (addr, fingerprint, server) = spawn_compute_server(&server_id, Some(8)).await;
    let handle = connect_client(addr, &server_id, &fingerprint, &client_id).await;
    let connection = handle.as_quic().expect("quic connection");

    let payload = vec![0x5Au8; 1024 * 1024];
    let request = TaskRequestFrame::new("dup-concurrent", payload.clone(), 15_000);

    // Submit the identical task on two streams at the same time.
    let mut submissions = Vec::new();
    for _ in 0..2 {
        let owned = owned_connection(connection);
        let request = request.clone();
        submissions.push(tokio::spawn(async move {
            let mut exchange = owned.open_task(&request).await?;
            let result = exchange
                .recv_result_with_timeout(Duration::from_secs(12))
                .await?;
            result.verify_for_request(&request)?;
            Ok::<TaskResultFrame, anyhow::Error>(result)
        }));
    }

    let mut results = Vec::new();
    for submission in submissions {
        results.push(submission.await.expect("join duplicate submission").expect(
            "a concurrent duplicate must receive the completed cached result, not an error",
        ));
    }

    assert!(results.iter().all(|result| result.success), "{results:?}");
    assert_eq!(results[0].result_hash, expected_sha256(&payload));
    assert_eq!(results[0].result_hash, results[1].result_hash);
    assert_eq!(results[0].result, results[1].result);

    let fresh = results.iter().filter(|result| !result.duplicate).count();
    let replayed = results.iter().filter(|result| result.duplicate).count();
    assert_eq!(fresh, 1, "exactly one submission must be a fresh execution");
    assert_eq!(replayed, 1, "the other submission must be a deduplicated replay");
    assert_eq!(
        server.execution_count(),
        1,
        "a concurrent duplicate must not execute the task twice"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stalled_stream_does_not_consume_sibling_timeout() {
    let server_id = unique_node("srv-timeout-iso");
    let client_id = unique_node("cli-timeout-iso");
    let (addr, fingerprint, _server) = spawn_compute_server(&server_id, Some(8)).await;
    let handle = connect_client(addr, &server_id, &fingerprint, &client_id).await;
    let connection = handle.as_quic().expect("quic connection");

    let mut stalled = open_stalled_stream(connection).await;

    let mut tasks = Vec::new();
    for index in 0..4u8 {
        let payload = vec![index; 256 * 1024];
        let request = TaskRequestFrame::new(format!("timeout-iso-{index}"), payload.clone(), 5_000);
        let owned = owned_connection(connection);
        tasks.push(tokio::spawn(async move {
            let start = Instant::now();
            let outcome = TaskClient::new(&owned)
                // Deliberately tight budget: only real isolation keeps this alive.
                .execute_with_timeout(&request, Duration::from_millis(1_000))
                .await;
            (payload, millis(start), outcome)
        }));
    }

    for task in tasks {
        let (payload, elapsed, outcome) = task.await.expect("join isolated task");
        let result = outcome.expect("a stalled sibling stream must not consume this budget");
        assert!(result.success, "{:?}", result.error_msg);
        assert_eq!(result.result_hash, expected_sha256(&payload));
        assert!(elapsed < 1_000.0, "isolated task took {elapsed:.1} ms");
    }

    let _ = stalled.reset(0u32.into());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn connection_interruption_isolation() {
    let server_id = unique_node("srv-interrupt");
    let client_id = unique_node("cli-interrupt");
    let (addr, fingerprint, server) = spawn_compute_server(&server_id, Some(8)).await;
    let handle = connect_client(addr, &server_id, &fingerprint, &client_id).await;
    let connection = handle.as_quic().expect("quic connection");

    let mut inflight = Vec::new();
    for index in 0..4u8 {
        let payload = vec![index; 2 * 1024 * 1024];
        let request = TaskRequestFrame::new(format!("interrupt-{index}"), payload, 15_000);
        let owned = owned_connection(connection);
        inflight.push(tokio::spawn(async move {
            let mut exchange = owned.open_task(&request).await?;
            exchange
                .recv_result_with_timeout(Duration::from_secs(10))
                .await
        }));
    }

    tokio::time::sleep(Duration::from_millis(30)).await;
    connection
        .connection
        .close(0u32.into(), b"concurrency test interruption");

    // In-flight tasks must fail cleanly, without panicking the runtime or the server.
    for task in inflight {
        let _ = task.await.expect("in-flight task must not panic");
    }

    // A new authenticated connection on the same server must still work.
    let other_client = unique_node("cli-interrupt-2");
    let handle2 = connect_client(addr, &server_id, &fingerprint, &other_client).await;
    let connection2 = handle2.as_quic().expect("second quic connection");
    let payload = b"after interruption".to_vec();
    let request = TaskRequestFrame::new("after-interruption", payload.clone(), 10_000);
    let result = TaskClient::new(connection2)
        .execute(&request)
        .await
        .expect("server must keep serving other connections");

    assert!(result.success, "{:?}", result.error_msg);
    assert_eq!(result.result_hash, expected_sha256(&payload));
    assert!(server.execution_count() >= 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn semaphore_permits_released_after_cancelled_streams() {
    let server_id = unique_node("srv-permits");
    let client_id = unique_node("cli-permits");
    let (addr, fingerprint, server) = spawn_compute_server(&server_id, Some(2)).await;
    let handle = connect_client(addr, &server_id, &fingerprint, &client_id).await;
    let connection = handle.as_quic().expect("quic connection");

    // Four cancelled streams must not leak admission permits.
    for _ in 0..4 {
        let (mut send, _recv) = connection.connection.open_bi().await.expect("open stream");
        send.write_all(&8192u32.to_be_bytes())
            .await
            .expect("write length");
        send.write_all(b"partial").await.expect("write partial");
        send.reset(0u32.into()).expect("reset stream");
    }
    tokio::time::sleep(Duration::from_millis(200)).await;

    // With only two permits, any leak would deadlock these and surface as a timeout.
    for index in 0..6u8 {
        let payload = vec![index; 64 * 1024];
        let request = TaskRequestFrame::new(format!("after-cancel-{index}"), payload.clone(), 5_000);
        let result = TaskClient::new(connection)
            .execute_with_timeout(&request, Duration::from_secs(3))
            .await
            .unwrap_or_else(|error| panic!("task {index} could not get a permit: {error}"));
        assert!(result.success, "{:?}", result.error_msg);
        assert_eq!(result.result_hash, expected_sha256(&payload));
    }

    assert!(
        server.peak_inflight() <= 2,
        "peak in-flight {} exceeded the configured limit",
        server.peak_inflight()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn inflight_limit_is_enforced() {
    let server_id = unique_node("srv-limit");
    let client_id = unique_node("cli-limit");
    let (addr, fingerprint, server) = spawn_compute_server(&server_id, Some(3)).await;
    let handle = connect_client(addr, &server_id, &fingerprint, &client_id).await;
    let connection = handle.as_quic().expect("quic connection");

    let mut tasks = Vec::new();
    for index in 0..8u8 {
        let payload = vec![index; 4 * 1024 * 1024];
        let request = TaskRequestFrame::new(format!("limit-{index}"), payload.clone(), 30_000);
        let owned = owned_connection(connection);
        tasks.push(tokio::spawn(async move {
            let outcome = TaskClient::new(&owned)
                .execute_with_timeout(&request, Duration::from_secs(25))
                .await;
            (payload, outcome)
        }));
    }

    for task in tasks {
        let (payload, outcome) = task.await.expect("join limited task");
        let result = outcome.expect("task under the admission limit");
        assert!(result.success, "{:?}", result.error_msg);
        assert_eq!(result.result_hash, expected_sha256(&payload));
    }

    let peak = server.peak_inflight();
    assert!(
        peak <= 3,
        "peak in-flight {peak} exceeded the configured limit of 3"
    );
    assert!(peak >= 2, "expected real concurrency, peak was {peak}");
    assert_eq!(server.execution_count(), 8);
}
