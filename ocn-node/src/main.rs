use std::sync::Arc;
use std::time::{Duration, Instant};
use std::collections::HashSet;
use clap::Parser;
use tracing::{info, error, warn};

use ocn_core::{NodeIdentity, QuicTransport, Transport, OCNPacket, OCNMsgType, generate_secure_nonce, DiscoveryRecord, platform_id, start_multicast_listener, advertise_discovery, browse_discovery};

#[derive(Parser, Debug)]
#[command(name = "ocn-node", version = "0.1.0", about = "OpenCompute Network Node Binary")]
struct Args {
    #[arg(long, default_value = "node-1")]
    node_id: String,

    #[arg(long, default_value = "127.0.0.1:9000")]
    listen: String,

    #[arg(long)]
    peer: Option<String>,

    #[arg(long, default_value = "ocn-secret-2026")]
    token: String,

    #[arg(long)]
    peer_fingerprint: Option<String>,

    #[arg(long, default_value_t = false)]
    server_mode: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();
    let args = Args::parse();

    println!("Starting OCN Node: {} on {}", args.node_id, args.listen);

    // 1. Persistent Cryptographic Identity Management
    let storage_dir = std::env::temp_dir().join("ocn_identities");
    let identity = NodeIdentity::load_or_generate(&args.node_id, &storage_dir)?;
    println!("Loaded Node Identity. Fingerprint: {}", identity.fingerprint);

    let transport = Arc::new(QuicTransport::new(
        identity,
        args.peer_fingerprint.clone(),
        args.token.clone(),
        format!("Windows-Node-{}", args.node_id),
        platform_id(),
        args.listen.split(':').nth(1).and_then(|p| p.parse().ok()).unwrap_or(9000),
        vec!["compute".into(), "storage".into()],
    ));

    // 2. Start Server
    let transport_server = transport.clone();
    let listen_addr = args.listen.clone();
    let expected_token = args.token.clone();

    let mut server_rx = transport_server.start_server(&listen_addr).await?;
    println!("OCN Node server successfully started and listening on {}", listen_addr);

    // Replay attack protection cache (nonces)
    let seen_nonces = Arc::new(tokio::sync::Mutex::new(HashSet::<u64>::new()));

    // 3. Start mDNS Discovery Tasks (Advertisement & Listening)
    let disc_node_id = args.node_id.clone();
    let disc_listen_addr = args.listen.clone();
    tokio::spawn(async move {
        let port = disc_listen_addr.split(':').nth(1).and_then(|p| p.parse::<u16>().ok()).unwrap_or(9000);
        let record = DiscoveryRecord::new(
            disc_node_id.clone(),
            format!("Windows-Node-{}", disc_node_id),
            "192.168.68.116".to_string(),
            port,
            1,
            platform_id(),
        );

        if let Ok(socket) = start_multicast_listener("0.0.0.0:5353".parse().unwrap()) {
            info!("mDNS discovery listener active on Windows node.");
            let socket = Arc::new(socket);

            // Task A: Periodic Advertisement
            let socket_adv = socket.clone();
            let rec_adv = record.clone();
            tokio::spawn(async move {
                loop {
                    if let Err(e) = advertise_discovery(rec_adv.clone(), &socket_adv).await {
                        warn!("mDNS discovery advertisement error: {}", e);
                    }
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            });

            // Task B: Browse Discovered Nodes
            let socket_browse = socket.clone();
            tokio::spawn(async move {
                loop {
                    if let Ok(Some(disc_rec)) = browse_discovery(&socket_browse).await {
                        info!("OCN_DISCOVERY_RECEIVED\nnode_id={}\nnode_name={}\nplatform_id={}\nip_address={}\nlisten_port={}",
                            disc_rec.node_id, disc_rec.node_name, disc_rec.platform_id, disc_rec.ip_address, disc_rec.listen_port);
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            });
        } else {
            warn!("Could not bind mDNS multicast port 5353 on Windows (may require admin / firewall permission). Sideload discovery active.");
        }
    });

    // Server connection handler loop
    tokio::spawn(async move {
        while let Some((packet, mut conn)) = server_rx.recv().await {
            let addr = conn.remote_addr();

            match packet.msg_type {
                OCNMsgType::AuthRequest { node_id, token, nonce } => {
                    info!("Received AuthRequest from node: {} at {} with nonce {}", node_id, addr, nonce);
                    
                    let mut nonces = seen_nonces.lock().await;
                    if nonces.contains(&nonce) {
                        warn!("Replay attack detected! Nonce {} already used.", nonce);
                        let resp = OCNPacket::new(OCNMsgType::AuthResponse {
                            success: false,
                            message: "Replay attack detected".into(),
                        });
                        let _ = conn.send(&resp).await;
                        continue;
                    }
                    nonces.insert(nonce);
                    drop(nonces);

                    let success = token == expected_token;
                    let resp = OCNPacket::new(OCNMsgType::AuthResponse {
                        success,
                        message: if success { "Authenticated".into() } else { "Invalid Token".into() },
                    });
                    if let Err(e) = conn.send(&resp).await {
                        error!("Failed to send auth response: {}", e);
                        continue;
                    }

                    if !success {
                        warn!("Authentication failed for node {}", node_id);
                        continue;
                    }
                    info!("Node {} successfully authenticated!", node_id);
                }
                OCNMsgType::Echo { payload } => {
                    info!("Echo received: {}", payload);
                    let reply = OCNPacket::new(OCNMsgType::Echo { payload });
                    let _ = conn.send(&reply).await;
                }
                OCNMsgType::HashTest { data, expected_hash } => {
                    use sha2::{Sha256, Digest};
                    let mut hasher = Sha256::new();
                    hasher.update(&data);
                    let computed = hex::encode(hasher.finalize());
                    let valid = computed == expected_hash;
                    info!("Hash integrity test: computed={}, expected={}, match={}", computed, expected_hash, valid);
                    let reply = OCNPacket::new(OCNMsgType::HashTestResult { success: valid, computed_hash: computed });
                    let _ = conn.send(&reply).await;
                }
                OCNMsgType::Ping { timestamp } => {
                    let reply = OCNPacket::new(OCNMsgType::Pong { timestamp });
                    let _ = conn.send(&reply).await;
                }
                OCNMsgType::ThroughputChunk { chunk_id, data: _ } => {
                    let reply = OCNPacket::new(OCNMsgType::ThroughputAck { chunk_id });
                    let _ = conn.send(&reply).await;
                }
                OCNMsgType::Disconnect => {
                    info!("Client requested disconnect.");
                }
                _ => {}
            }
        }
    });

    // 3. Client Mode & Test Bench Execution
    if let Some(peer_addr) = args.peer {
        println!("Connecting to peer at {}...", peer_addr);
        let mut conn_opt = None;
        for attempt in 1..=5 {
            match transport.connect(&peer_addr).await {
                Ok(c) => {
                    println!("Successfully connected to peer at {}", peer_addr);
                    conn_opt = Some(c);
                    break;
                }
                Err(e) => {
                    println!("Connection attempt {}/5 failed: {}. Retrying in 1s...", attempt, e);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }

        if let Some(mut client_conn) = conn_opt {
            let nonce = generate_secure_nonce();
            println!("Sending Authentication Request with CSPRNG nonce {}...", nonce);
            let auth_req = OCNPacket::new(OCNMsgType::AuthRequest {
                node_id: args.node_id.clone(),
                token: args.token.clone(),
                nonce,
            });
            client_conn.send(&auth_req).await?;

            if let Some(resp) = client_conn.recv().await? {
                match resp.msg_type {
                    OCNMsgType::AuthResponse { success, message } => {
                        if !success {
                            error!("Authentication rejected by peer: {}", message);
                            return Ok(());
                        }
                        println!("Authentication accepted: {}", message);
                    }
                    _ => {
                        error!("Unexpected auth response");
                        return Ok(());
                    }
                }
            }

            // --- Echo Test ---
            println!("=== Running Echo Test ===");
            let echo_msg = "Hello OCN Distributed Network!".to_string();
            let start = Instant::now();
            client_conn.send(&OCNPacket::new(OCNMsgType::Echo { payload: echo_msg.clone() })).await?;
            if let Some(resp) = client_conn.recv().await? {
                if let OCNMsgType::Echo { payload } = resp.msg_type {
                    let duration = start.elapsed();
                    println!("Echo Success! Payload: '{}', Round-trip time: {:?}", payload, duration);
                    assert_eq!(payload, echo_msg);
                }
            }

            // --- Hash Data Integrity Test ---
            println!("=== Running Hash Integrity Test ===");
            let test_data = b"OpenCompute Network secure data payload for hash verification.".to_vec();
            use sha2::{Sha256, Digest};
            let mut hasher = Sha256::new();
            hasher.update(&test_data);
            let expected_hash = hex::encode(hasher.finalize());

            client_conn.send(&OCNPacket::new(OCNMsgType::HashTest { data: test_data, expected_hash: expected_hash.clone() })).await?;

            if let Some(resp) = client_conn.recv().await? {
                if let OCNMsgType::HashTestResult { success, computed_hash } = resp.msg_type {
                    println!("Hash Test Result: success={}, computed={}", success, computed_hash);
                    assert!(success);
                }
            }

            // --- RTT Latency Benchmarking (P50, P95, P99 Calculation) ---
            println!("=== Running Detailed RTT Latency Benchmark (100 samples) ===");
            let mut rtt_samples = Vec::new();
            for _sample_idx in 0..100 {
                let ping_time = Instant::now().elapsed().as_nanos() as u64;
                let start_ping = Instant::now();
                client_conn.send(&OCNPacket::new(OCNMsgType::Ping { timestamp: ping_time })).await?;
                if let Some(resp) = client_conn.recv().await? {
                    if let OCNMsgType::Pong { timestamp: _ } = resp.msg_type {
                        let rtt = start_ping.elapsed().as_secs_f64() * 1000.0;
                        rtt_samples.push(rtt);
                    }
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            rtt_samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let avg_rtt: f64 = rtt_samples.iter().sum::<f64>() / rtt_samples.len() as f64;
            let p50 = rtt_samples[49];
            let p95 = rtt_samples[94];
            let p99 = rtt_samples[98];
            println!("RTT Benchmark complete. Avg: {:.3} ms, P50: {:.3} ms, P95: {:.3} ms, P99: {:.3} ms", avg_rtt, p50, p95, p99);

            // --- Throughput Test ---
            println!("=== Running Throughput Bandwidth Test ===");
            let chunk_size = 64 * 1024; // 64 KB
            let total_chunks = 50; // 3.2 MB total
            let total_bytes = chunk_size * total_chunks;
            let dummy_data = vec![0xABu8; chunk_size];

            let throughput_start = Instant::now();
            client_conn.send(&OCNPacket::new(OCNMsgType::ThroughputStart { chunk_size, total_chunks })).await?;

            for chunk_id in 0..total_chunks {
                client_conn.send(&OCNPacket::new(OCNMsgType::ThroughputChunk { chunk_id, data: dummy_data.clone() })).await?;
                let _ = client_conn.recv().await?;
            }
            let throughput_duration = throughput_start.elapsed();
            let mbps = (total_bytes as f64 * 8.0) / (throughput_duration.as_secs_f64() * 1_000_000.0);
            println!("Throughput Test Completed: Transferred {} bytes in {:?}, Bandwidth: {:.2} Mbps", total_bytes, throughput_duration, mbps);

            println!("All Phase 2.0 tests completed successfully!");
        }
    } else {
        loop {
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    }

    Ok(())
}
