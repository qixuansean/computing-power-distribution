//! Real-device OCN compute server and client.
//!
//! One binary, two roles:
//!
//! ```text
//! ocn-compute server --node-id compute-b-01 --listen 0.0.0.0:9100
//! ocn-compute client --server 192.168.68.x:9100 \
//!     --server-node-id compute-b-01 --server-fingerprint <sha256-hex>
//! ```
//!
//! The server wraps the existing `TaskServer` + `QuicTransport`; the client wraps the
//! existing `TaskClient` + `QuicTransport`. The B-2 handshake, TLS certificate pinning,
//! task wire protocol and `ComputeExecutor` are reused unchanged. Nothing here disables
//! TLS verification or bypasses B-2 identity checks.
//!
//! Secrets: the pairing token is read from an environment variable by default. It is
//! never printed. Identity private keys stay in the identity directory and are never
//! logged. Only the public certificate fingerprint is printed, for out-of-band pairing.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use clap::{Args, Parser, Subcommand, ValueEnum};
use ocn_compute::server::{TaskClient, TaskServer};
use ocn_core::identity::{get_pairing_store, NodeIdentity};
use ocn_core::task_wire::{error_code, read_task_frame};
use ocn_core::{
    platform_id, ConnectionState, QuicConnection, QuicTransport, TaskRequestFrame,
    TaskResultFrame, Transport,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

const DEFAULT_ENV_TOKEN: &str = "OCN_PAIRING_TOKEN";
const DEFAULT_IDENTITY_DIR: &str = "ocn_identities";
const TASK_TIMEOUT_MS: u64 = 30_000;
const CONNECT_TIMEOUT_SECS: u64 = 15;

#[derive(Parser, Debug)]
#[command(
    name = "ocn-compute",
    version,
    about = "OCN real-device compute server/client over B-2 authenticated QUIC"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run the OCN compute server that executes BatchHash tasks.
    Server(ServerArgs),
    /// Run the OCN compute client that submits tasks and verifies results.
    Client(ClientArgs),
}

#[derive(Args, Debug)]
struct ServerArgs {
    /// OCN node id advertised during the B-2 handshake.
    #[arg(long, default_value = "compute-b-01")]
    node_id: String,

    /// UDP address to bind. Defaults to loopback; use a LAN address only in a controlled network.
    #[arg(long, default_value = "127.0.0.1:9100")]
    listen: String,

    /// Directory holding the persistent certificate and key.
    #[arg(long)]
    identity_dir: Option<PathBuf>,

    /// Human readable node name sent in NodeInfo.
    #[arg(long, default_value = "OCN-Compute-Server")]
    name: String,

    /// Environment variable holding the pairing token (preferred).
    #[arg(long, default_value = DEFAULT_ENV_TOKEN)]
    token_env: String,

    /// Pairing token on the command line. Visible to other local processes; prefer --token-env.
    #[arg(long)]
    token: Option<String>,

    /// How often to print the executed-task counter.
    #[arg(long, default_value_t = 10)]
    stats_interval_secs: u64,
}

#[derive(Args, Debug)]
struct ClientArgs {
    /// Server address, for example 192.168.68.114:9100.
    #[arg(long)]
    server: String,

    /// OCN node id of the server, obtained out-of-band.
    #[arg(long)]
    server_node_id: String,

    /// SHA-256 certificate fingerprint of the server, obtained out-of-band (64 hex chars).
    #[arg(long)]
    server_fingerprint: String,

    /// OCN node id of this client.
    #[arg(long, default_value = "compute-a-01")]
    node_id: String,

    /// Directory holding the persistent certificate and key.
    #[arg(long)]
    identity_dir: Option<PathBuf>,

    /// Human readable node name sent in NodeInfo.
    #[arg(long, default_value = "OCN-Compute-Client")]
    name: String,

    /// Environment variable holding the pairing token (preferred).
    #[arg(long, default_value = DEFAULT_ENV_TOKEN)]
    token_env: String,

    /// Pairing token on the command line. Visible to other local processes; prefer --token-env.
    #[arg(long)]
    token: Option<String>,

    /// Scenario to run.
    #[arg(long, value_enum, default_value = "all")]
    scenario: Scenario,

    /// Evidence label written to the report, for example dual-device or single-device-two-process.
    #[arg(long, default_value = "unlabeled")]
    label: String,

    /// Optional path for a JSON metrics report.
    #[arg(long)]
    report: Option<PathBuf>,

    /// Maximum time to wait for the authenticated connection.
    #[arg(long, default_value_t = CONNECT_TIMEOUT_SECS)]
    connect_timeout_secs: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Scenario {
    All,
    Small,
    #[value(name = "256k")]
    Payload256k,
    #[value(name = "1m")]
    Payload1m,
    Concurrent,
    Duplicate,
    Invalid,
    Timeout,
}

impl Scenario {
    fn name(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Small => "small",
            Self::Payload256k => "256k",
            Self::Payload1m => "1m",
            Self::Concurrent => "concurrent",
            Self::Duplicate => "duplicate",
            Self::Invalid => "invalid",
            Self::Timeout => "timeout",
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Server(args) => run_server(args).await,
        Command::Client(args) => run_client(args).await,
    }
}

async fn run_server(args: ServerArgs) -> anyhow::Result<()> {
    let token = resolve_token(&args.token_env, args.token.as_deref())?;
    let socket: SocketAddr = args
        .listen
        .parse()
        .with_context(|| format!("invalid --listen address '{}'", args.listen))?;

    let identity_dir = args
        .identity_dir
        .clone()
        .unwrap_or_else(default_identity_dir);
    let identity = NodeIdentity::load_or_generate(&args.node_id, &identity_dir)
        .context("load or generate server identity")?;

    let transport = QuicTransport::new(
        identity.clone(),
        None,
        token,
        args.name,
        platform_id(),
        socket.port(),
        vec!["compute".to_string()],
    );

    let mut rx = transport
        .start_server(&args.listen)
        .await
        .with_context(|| format!("start QUIC server on {}", args.listen))?;

    println!(
        "OCN_COMPUTE_SERVER_READY node_id={} fingerprint={} listen={}",
        args.node_id, identity.fingerprint, socket
    );
    if !socket.ip().is_loopback() {
        println!(
            "OCN_COMPUTE_SERVER_NETWORK_WARNING bind={} is not loopback; allow this UDP port only on the private/controlled network",
            socket
        );
    }

    let server = Arc::new(TaskServer::new(args.node_id.clone()));
    let stats_server = server.clone();
    let stats_interval = Duration::from_secs(args.stats_interval_secs.max(1));
    let stats_task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(stats_interval).await;
            println!(
                "OCN_SERVER_STATS executed_total={}",
                stats_server.execution_count()
            );
        }
    });

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("OCN_COMPUTE_SERVER_STOPPING");
                break;
            }
            incoming = rx.recv() => {
                let Some((_packet, handle)) = incoming else { break };
                let peer = handle.remote_addr();
                let peer_node = handle.as_quic().and_then(|connection| connection.peer_node_id.clone());
                println!(
                    "OCN_CLIENT_AUTHENTICATED peer={} peer_node_id={}",
                    peer,
                    peer_node.unwrap_or_else(|| "unknown".to_string())
                );

                let server = server.clone();
                tokio::spawn(async move {
                    if let Some(connection) = handle.as_quic() {
                        server.serve_connection(connection).await;
                    }
                    println!("OCN_CLIENT_DISCONNECTED peer={}", peer);
                });
            }
        }
    }

    stats_task.abort();
    println!(
        "OCN_COMPUTE_SERVER_STOPPED executed_total={}",
        server.execution_count()
    );
    Ok(())
}

async fn run_client(args: ClientArgs) -> anyhow::Result<()> {
    validate_fingerprint(&args.server_fingerprint)?;
    let token = resolve_token(&args.token_env, args.token.as_deref())?;
    let identity_dir = args
        .identity_dir
        .clone()
        .unwrap_or_else(default_identity_dir);
    let identity = NodeIdentity::load_or_generate(&args.node_id, &identity_dir)
        .context("load or generate client identity")?;

    // Out-of-band trust anchor: only this exact (node_id, certificate fingerprint) pair
    // is accepted for the server. B-2 will reject anything else.
    {
        let mut store = get_pairing_store().lock().expect("pairing store lock");
        store.node_id_to_fingerprint.insert((
            args.server_node_id.clone(),
            args.server_fingerprint.clone(),
        ));
    }

    let transport = QuicTransport::new(
        identity.clone(),
        Some(args.server_fingerprint.clone()),
        token,
        args.name,
        platform_id(),
        0,
        vec![],
    );

    let connect_start = Instant::now();
    let handle = tokio::time::timeout(
        Duration::from_secs(args.connect_timeout_secs.max(1)),
        transport.connect(&args.server),
    )
    .await
    .context("timed out waiting for the B-2 authenticated connection")?
    .context("connect and authenticate against server")?;
    let connect_ms = elapsed_ms(connect_start);

    let connection = handle
        .as_quic()
        .ok_or_else(|| anyhow::anyhow!("transport did not return a QUIC connection"))?;

    println!(
        "OCN_CLIENT_CONNECTED server={} server_node_id={} client_node_id={} connect_ms={:.3}",
        args.server, args.server_node_id, args.node_id, connect_ms
    );

    let run_id = format!(
        "{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );

    // One untimed warm-up task absorbs first-task cold-start effects (thread pool,
    // firewall/AV inspection, QUIC stream setup) so scenario numbers are comparable.
    // Its latency is still recorded separately in the report.
    let (warmup_ms, warmup_passed) = run_warmup(connection, &run_id).await;
    println!(
        "OCN_CLIENT_WARMUP latency_ms={:.3} passed={}",
        warmup_ms, warmup_passed
    );
    if !warmup_passed {
        anyhow::bail!("warm-up task failed; refusing to report benchmark numbers");
    }

    let scenarios: Vec<Scenario> = if args.scenario == Scenario::All {
        vec![
            Scenario::Small,
            Scenario::Payload256k,
            Scenario::Payload1m,
            Scenario::Concurrent,
            Scenario::Duplicate,
            Scenario::Invalid,
            Scenario::Timeout,
        ]
    } else {
        vec![args.scenario]
    };

    let mut recorder = Recorder::default();
    let total_start = Instant::now();

    for scenario in scenarios {
        let scenario_start = Instant::now();
        match scenario {
            Scenario::All => unreachable!("all is expanded before dispatch"),
            Scenario::Small => {
                run_payload_case(
                    connection,
                    scenario,
                    format!("{run_id}-small"),
                    b"OCN MS-Real-Device BatchHash: small UTF-8 text payload.".to_vec(),
                    &mut recorder,
                )
                .await;
            }
            Scenario::Payload256k => {
                run_payload_case(
                    connection,
                    scenario,
                    format!("{run_id}-256k"),
                    patterned_payload(256 * 1024, 0x256),
                    &mut recorder,
                )
                .await;
            }
            Scenario::Payload1m => {
                run_payload_case(
                    connection,
                    scenario,
                    format!("{run_id}-1m"),
                    patterned_payload(1024 * 1024, 0x1_000_000),
                    &mut recorder,
                )
                .await;
            }
            Scenario::Concurrent => {
                run_concurrent(connection, &run_id, &mut recorder).await;
            }
            Scenario::Duplicate => {
                run_duplicate(connection, &run_id, &mut recorder).await;
            }
            Scenario::Invalid => {
                run_invalid(connection, &run_id, &mut recorder).await;
            }
            Scenario::Timeout => {
                run_timeout(connection, &run_id, &mut recorder).await;
            }
        }
        let summary = recorder.summarize(scenario.name(), scenario_start.elapsed());
        println!(
            "SCENARIO {} tasks={} passed={} failed={} bytes={} wall_ms={:.3} min_ms={:.3} avg_ms={:.3} max_ms={:.3} throughput_mib_s={:.2}",
            summary.name,
            summary.tasks,
            summary.passed,
            summary.failed,
            summary.bytes,
            summary.wall_ms,
            summary.min_latency_ms,
            summary.avg_latency_ms,
            summary.max_latency_ms,
            summary.throughput_mib_s
        );
    }

    let total_wall_ms = elapsed_ms(total_start);
    let total_bytes: usize = recorder.tasks.iter().map(|record| record.bytes).sum();
    let failed = recorder.tasks.iter().filter(|record| !record.passed).count();
    let overall_mib_s = if total_wall_ms > 0.0 {
        mib_per_sec(total_bytes, total_wall_ms)
    } else {
        0.0
    };

    let report = Report {
        label: args.label.clone(),
        host: std::env::var("COMPUTERNAME").unwrap_or_else(|_| "unknown".to_string()),
        server: args.server.clone(),
        server_node_id: args.server_node_id.clone(),
        client_node_id: args.node_id.clone(),
        connect_ms,
        warmup_ms,
        warmup_passed,
        run_id: run_id.clone(),
        started_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        total_tasks: recorder.tasks.len(),
        passed_tasks: recorder.tasks.len() - failed,
        failed_tasks: failed,
        total_bytes,
        total_wall_ms,
        overall_mib_s,
        scenarios: recorder.summaries.clone(),
        tasks: recorder.tasks.clone(),
        failures: recorder.failures.clone(),
    };

    if args.label == "unlabeled" {
        println!("OCN_CLIENT_WARNING label=unlabeled; pass --label to identify the evidence type");
    }

    println!(
        "CLIENT_SUMMARY label={} tasks={} passed={} failed={} bytes={} wall_ms={:.3} overall_mib_s={:.2} connect_ms={:.3} warmup_ms={:.3}",
        report.label,
        report.total_tasks,
        report.passed_tasks,
        report.failed_tasks,
        report.total_bytes,
        report.total_wall_ms,
        report.overall_mib_s,
        report.connect_ms,
        report.warmup_ms
    );

    for failure in &recorder.failures {
        println!("FAILURE {}", sanitize(failure));
    }

    if let Some(path) = &args.report {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).ok();
            }
        }
        let json = serde_json::to_vec_pretty(&report)?;
        std::fs::write(path, json)
            .with_context(|| format!("write report to {}", path.display()))?;
        println!("REPORT_WRITTEN path={}", path.display());
    }

    if failed > 0 {
        std::process::exit(2);
    }
    Ok(())
}

async fn run_warmup(connection: &QuicConnection, run_id: &str) -> (f64, bool) {
    let payload = b"OCN warm-up task".to_vec();
    let request = TaskRequestFrame::new(
        format!("{run_id}-warmup"),
        payload.clone(),
        TASK_TIMEOUT_MS,
    );
    let expected_hash = sha256(&payload);
    let start = Instant::now();
    match TaskClient::new(connection).execute(&request).await {
        Ok(result) => (
            elapsed_ms(start),
            result.success && result.result == payload && result.result_hash == expected_hash,
        ),
        Err(_) => (elapsed_ms(start), false),
    }
}

async fn run_payload_case(
    connection: &QuicConnection,
    scenario: Scenario,
    task_id: String,
    payload: Vec<u8>,
    recorder: &mut Recorder,
) {
    let client = TaskClient::new(connection);
    let request = TaskRequestFrame::new(task_id.clone(), payload.clone(), TASK_TIMEOUT_MS);
    let expected_hash = sha256(&payload);

    let start = Instant::now();
    let outcome = client.execute(&request).await;
    let latency_ms = elapsed_ms(start);

    match outcome {
        Ok(result) => {
            let hash_matches = result.result_hash == expected_hash;
            let result_matches = result.result == payload;
            let passed = result.success && hash_matches && result_matches;
            recorder.record(TaskRecord {
                scenario: scenario.name().to_string(),
                task_id,
                bytes: payload.len(),
                passed,
                server_success: Some(result.success),
                duplicate: result.duplicate,
                latency_ms,
                server_execution_ms: result.execution_time_ms,
                throughput_mib_s: mib_per_sec(payload.len(), latency_ms),
                error_code: result.error_code,
                error: if passed {
                    None
                } else if !result.success {
                    result.error_msg
                } else if !hash_matches {
                    Some("client re-computed SHA-256 does not match result_hash".to_string())
                } else {
                    Some("result bytes do not match the submitted payload".to_string())
                },
            });
        }
        Err(error) => {
            recorder.record(TaskRecord {
                scenario: scenario.name().to_string(),
                task_id,
                bytes: payload.len(),
                passed: false,
                server_success: None,
                duplicate: false,
                latency_ms,
                server_execution_ms: 0,
                throughput_mib_s: mib_per_sec(payload.len(), latency_ms),
                error_code: None,
                error: Some(error.to_string()),
            });
        }
    }
}

async fn run_concurrent(connection: &QuicConnection, run_id: &str, recorder: &mut Recorder) {
    let mut handles = Vec::new();
    for index in 0..8u8 {
        let payload = patterned_payload(256 * 1024, 0xC0_0000 + index as u64);
        let request = TaskRequestFrame::new(
            format!("{run_id}-concurrent-{index}"),
            payload.clone(),
            30_000,
        );
        let owned = owned_connection(connection);
        handles.push(tokio::spawn(async move {
            let expected_hash = sha256(&payload);
            let start = Instant::now();
            let outcome = TaskClient::new(&owned).execute(&request).await;
            (payload, expected_hash, elapsed_ms(start), outcome)
        }));
    }

    for handle in handles {
        match handle.await {
            Ok((payload, expected_hash, latency_ms, Ok(result))) => {
                let passed = result.success
                    && result.result == payload
                    && result.result_hash == expected_hash;
                recorder.record(TaskRecord {
                    scenario: "concurrent".to_string(),
                    task_id: result.task_id,
                    bytes: payload.len(),
                    passed,
                    server_success: Some(result.success),
                    duplicate: result.duplicate,
                    latency_ms,
                    server_execution_ms: result.execution_time_ms,
                    throughput_mib_s: mib_per_sec(payload.len(), latency_ms),
                    error_code: result.error_code,
                    error: if passed {
                        None
                    } else {
                        Some(result.error_msg.unwrap_or_else(|| "result verification failed".to_string()))
                    },
                });
            }
            Ok((payload, _expected_hash, latency_ms, Err(error))) => {
                recorder.record(TaskRecord {
                    scenario: "concurrent".to_string(),
                    task_id: "concurrent-task".to_string(),
                    bytes: payload.len(),
                    passed: false,
                    server_success: None,
                    duplicate: false,
                    latency_ms,
                    server_execution_ms: 0,
                    throughput_mib_s: mib_per_sec(payload.len(), latency_ms),
                    error_code: None,
                    error: Some(error.to_string()),
                });
            }
            Err(join_error) => {
                recorder.record(TaskRecord {
                    scenario: "concurrent".to_string(),
                    task_id: "concurrent-task".to_string(),
                    bytes: 0,
                    passed: false,
                    server_success: None,
                    duplicate: false,
                    latency_ms: 0.0,
                    server_execution_ms: 0,
                    throughput_mib_s: 0.0,
                    error_code: None,
                    error: Some(format!("concurrent task join failed: {join_error}")),
                });
            }
        }
    }
}

async fn run_duplicate(connection: &QuicConnection, run_id: &str, recorder: &mut Recorder) {
    let client = TaskClient::new(connection);
    let payload = b"OCN duplicate task payload".to_vec();
    let task_id = format!("{run_id}-duplicate");
    let request = TaskRequestFrame::new(task_id.clone(), payload.clone(), TASK_TIMEOUT_MS);
    let expected_hash = sha256(&payload);

    let start = Instant::now();
    let first = client.execute(&request).await;
    let first_latency = elapsed_ms(start);

    let start = Instant::now();
    let second = client.execute(&request).await;
    let second_latency = elapsed_ms(start);

    let (first_passed, first_duplicate, first_code, first_error) = match &first {
        Ok(result) => (
            result.success && result.result_hash == expected_hash && !result.duplicate,
            result.duplicate,
            result.error_code,
            result.error_msg.clone(),
        ),
        Err(error) => (false, false, None, Some(error.to_string())),
    };
    recorder.record(TaskRecord {
        scenario: "duplicate".to_string(),
        task_id: format!("{task_id}#first"),
        bytes: payload.len(),
        passed: first_passed,
        server_success: first.as_ref().ok().map(|result| result.success),
        duplicate: first_duplicate,
        latency_ms: first_latency,
        server_execution_ms: first
            .as_ref()
            .map(|result| result.execution_time_ms)
            .unwrap_or(0),
        throughput_mib_s: mib_per_sec(payload.len(), first_latency),
        error_code: first_code,
        error: if first_passed {
            None
        } else {
            first_error.or_else(|| Some("first execution was not a fresh success".to_string()))
        },
    });

    let second_passed = match (&first, &second) {
        (Ok(first), Ok(second)) => {
            second.success
                && second.duplicate
                && second.result_hash == expected_hash
                && second.result_hash == first.result_hash
        }
        _ => false,
    };
    recorder.record(TaskRecord {
        scenario: "duplicate".to_string(),
        task_id: format!("{task_id}#second"),
        bytes: payload.len(),
        passed: second_passed,
        server_success: second.as_ref().ok().map(|result| result.success),
        duplicate: second.as_ref().ok().map(|result| result.duplicate).unwrap_or(false),
        latency_ms: second_latency,
        server_execution_ms: second
            .as_ref()
            .map(|result| result.execution_time_ms)
            .unwrap_or(0),
        throughput_mib_s: mib_per_sec(payload.len(), second_latency),
        error_code: second.as_ref().ok().and_then(|result| result.error_code),
        error: if second_passed {
            None
        } else {
            second
                .as_ref()
                .err()
                .map(|error| error.to_string())
                .or_else(|| Some("second execution was not a deduplicated replay".to_string()))
        },
    });
}

async fn run_invalid(connection: &QuicConnection, run_id: &str, recorder: &mut Recorder) {
    let client = TaskClient::new(connection);
    let mut request = TaskRequestFrame::new(
        format!("{run_id}-invalid"),
        b"invalid input hash payload".to_vec(),
        10_000,
    );
    request.input_hash = [0u8; 32];

    let start = Instant::now();
    let outcome = client.execute(&request).await;
    let latency_ms = elapsed_ms(start);

    let (passed, success, code, message) = match outcome {
        Ok(result) => (
            !result.success && result.error_code == Some(error_code::INVALID_TASK),
            Some(result.success),
            result.error_code,
            result.error_msg,
        ),
        Err(error) => (false, None, None, Some(error.to_string())),
    };

    recorder.record(TaskRecord {
        scenario: "invalid".to_string(),
        task_id: request.task_id.clone(),
        bytes: request.payload.len(),
        passed,
        server_success: success,
        duplicate: false,
        latency_ms,
        server_execution_ms: 0,
        throughput_mib_s: mib_per_sec(request.payload.len(), latency_ms),
        error_code: code,
        error: if passed {
            None
        } else {
            message.or_else(|| Some("expected INVALID_TASK rejection".to_string()))
        },
    });
}

async fn run_timeout(connection: &QuicConnection, run_id: &str, recorder: &mut Recorder) {
    // Deterministic timeout probe: open a task stream and send an incomplete frame
    // (length prefix announces 1024 bytes, only 7 are written). The server waits for
    // the rest of the body, so a bounded read cannot complete and must time out.
    let announced_len = 1024usize;
    let partial_body = b"partial";
    let start = Instant::now();
    let outcome: anyhow::Result<()> = async {
        let (mut send, mut recv) = connection.connection.open_bi().await?;
        send.write_all(&(announced_len as u32).to_be_bytes()).await?;
        send.write_all(partial_body).await?;
        let _: TaskResultFrame = read_task_frame(&mut recv, Duration::from_millis(300)).await?;
        Ok(())
    }
    .await;
    let latency_ms = elapsed_ms(start);

    // Dropping the incomplete stream halves implicitly resets them.
    let (passed, error) = match outcome {
        Err(error) if error.to_string().contains("timed out") => (true, None),
        Err(error) => (false, Some(format!("unexpected timeout error: {error}"))),
        Ok(()) => (
            false,
            Some("expected a bounded read timeout but a result arrived".to_string()),
        ),
    };

    recorder.record(TaskRecord {
        scenario: "timeout".to_string(),
        task_id: format!("{run_id}-timeout"),
        bytes: 4 + partial_body.len(),
        passed,
        server_success: None,
        duplicate: false,
        latency_ms,
        server_execution_ms: 0,
        throughput_mib_s: 0.0,
        error_code: None,
        error,
    });

    // The timed-out stream must not poison the connection: a follow-up task must succeed.
    let recovery_payload = b"timeout recovery payload".to_vec();
    run_payload_case(
        connection,
        Scenario::Timeout,
        format!("{run_id}-timeout-recovery"),
        recovery_payload,
        recorder,
    )
    .await;
}

#[derive(Clone, Debug, Serialize)]
struct TaskRecord {
    scenario: String,
    task_id: String,
    bytes: usize,
    passed: bool,
    server_success: Option<bool>,
    duplicate: bool,
    latency_ms: f64,
    server_execution_ms: u64,
    throughput_mib_s: f64,
    error_code: Option<u32>,
    error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct ScenarioSummary {
    name: String,
    tasks: usize,
    passed: usize,
    failed: usize,
    bytes: usize,
    wall_ms: f64,
    min_latency_ms: f64,
    avg_latency_ms: f64,
    max_latency_ms: f64,
    throughput_mib_s: f64,
}

#[derive(Serialize)]
struct Report {
    label: String,
    host: String,
    server: String,
    server_node_id: String,
    client_node_id: String,
    connect_ms: f64,
    warmup_ms: f64,
    warmup_passed: bool,
    run_id: String,
    started_unix_ms: u128,
    total_tasks: usize,
    passed_tasks: usize,
    failed_tasks: usize,
    total_bytes: usize,
    total_wall_ms: f64,
    overall_mib_s: f64,
    scenarios: Vec<ScenarioSummary>,
    tasks: Vec<TaskRecord>,
    failures: Vec<String>,
}

#[derive(Default)]
struct Recorder {
    tasks: Vec<TaskRecord>,
    summaries: Vec<ScenarioSummary>,
    failures: Vec<String>,
}

impl Recorder {
    fn record(&mut self, record: TaskRecord) {
        println!(
            "TASK scenario={} task_id={} bytes={} passed={} server_success={} duplicate={} latency_ms={:.3} server_execution_ms={} throughput_mib_s={:.2} error_code={} error={}",
            record.scenario,
            record.task_id,
            record.bytes,
            record.passed,
            record
                .server_success
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            record.duplicate,
            record.latency_ms,
            record.server_execution_ms,
            record.throughput_mib_s,
            record
                .error_code
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            sanitize(record.error.as_deref().unwrap_or(""))
        );
        if !record.passed {
            self.failures.push(format!(
                "{} [{}]: {}",
                record.task_id,
                record.scenario,
                record.error.clone().unwrap_or_else(|| "unknown failure".to_string())
            ));
        }
        self.tasks.push(record);
    }

    fn summarize(&mut self, name: &str, wall: Duration) -> ScenarioSummary {
        let records: Vec<&TaskRecord> = self
            .tasks
            .iter()
            .filter(|record| record.scenario == name)
            .collect();
        let tasks = records.len();
        let passed = records.iter().filter(|record| record.passed).count();
        let failed = tasks - passed;
        let bytes: usize = records.iter().map(|record| record.bytes).sum();
        let wall_ms = wall.as_secs_f64() * 1000.0;
        let mut latencies: Vec<f64> = records.iter().map(|record| record.latency_ms).collect();
        latencies.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let min_latency_ms = latencies.first().copied().unwrap_or(0.0);
        let max_latency_ms = latencies.last().copied().unwrap_or(0.0);
        let avg_latency_ms = if latencies.is_empty() {
            0.0
        } else {
            latencies.iter().sum::<f64>() / latencies.len() as f64
        };
        let throughput_mib_s = if wall_ms > 0.0 {
            mib_per_sec(bytes, wall_ms)
        } else {
            0.0
        };

        let summary = ScenarioSummary {
            name: name.to_string(),
            tasks,
            passed,
            failed,
            bytes,
            wall_ms,
            min_latency_ms,
            avg_latency_ms,
            max_latency_ms,
            throughput_mib_s,
        };
        self.summaries.push(summary.clone());
        summary
    }
}

fn owned_connection(connection: &QuicConnection) -> QuicConnection {
    QuicConnection {
        connection: connection.connection.clone(),
        state: ConnectionState::Active,
        peer_node_id: connection.peer_node_id.clone(),
    }
}

fn resolve_token(env_name: &str, cli_token: Option<&str>) -> anyhow::Result<String> {
    if let Ok(value) = std::env::var(env_name) {
        let value = value.trim().to_string();
        if !value.is_empty() {
            return Ok(value);
        }
    }
    if let Some(value) = cli_token {
        let value = value.trim().to_string();
        if !value.is_empty() {
            eprintln!(
                "WARNING: --token exposes the pairing token in process arguments; prefer the {} environment variable",
                env_name
            );
            return Ok(value);
        }
    }
    anyhow::bail!(
        "pairing token is missing: set the {} environment variable (preferred) or pass --token",
        env_name
    )
}

fn validate_fingerprint(fingerprint: &str) -> anyhow::Result<()> {
    if fingerprint.len() != 64 || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        anyhow::bail!(
            "--server-fingerprint must be the 64-character hex SHA-256 fingerprint of the server certificate"
        );
    }
    Ok(())
}

fn default_identity_dir() -> PathBuf {
    std::env::temp_dir().join(DEFAULT_IDENTITY_DIR)
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn patterned_payload(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed | 1;
    let mut payload = Vec::with_capacity(len);
    for _ in 0..len {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        payload.push((state & 0xff) as u8);
    }
    payload
}

fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn mib_per_sec(bytes: usize, latency_ms: f64) -> f64 {
    if latency_ms <= 0.0 {
        return 0.0;
    }
    (bytes as f64 / (1024.0 * 1024.0)) / (latency_ms / 1000.0)
}

fn sanitize(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}
