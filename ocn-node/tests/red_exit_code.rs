//! RED test for Phase 3.1 — B-3: a failed connection must not exit with status 0.
//!
//! **This test is expected to FAIL against the current implementation.**
//! `ocn-node/src/main.rs` returns `Ok(())` when every connection attempt fails
//! (see `main.rs:199` `conn_opt` stays `None` and `main.rs:303` returns `Ok(())`),
//! so automation and CI cannot detect the failure.
//!
//! Run with:
//!   cargo test -p ocn-node --test red_exit_code -- --nocapture

use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

use ocn_core::{NodeIdentity, QuicTransport, Transport};

static NEXT_PORT: AtomicU16 = AtomicU16::new(44200);

fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join(" | ")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn red_b3_binary_exits_nonzero_after_auth_failure() {
    // The server deliberately uses a *different* pairing token, so authentication
    // must keep failing even after the B-1 identity fix lands. This makes the test
    // about the exit code only, not about the handshake.
    let identity = NodeIdentity::generate("node-red-rejector").expect("server identity");

    let mut bound = None;
    for _ in 0..20 {
        let port = NEXT_PORT.fetch_add(1, Ordering::SeqCst);
        let addr = format!("127.0.0.1:{port}");
        let transport = QuicTransport::new(
            identity.clone(),
            None,
            "definitely-not-the-client-token".to_string(),
            "red-rejector".to_string(),
            "red-test".to_string(),
            port,
            vec![],
        );
        if let Ok(rx) = transport.start_server(&addr).await {
            bound = Some((addr, rx));
            break;
        }
    }
    let (addr, _rx) = bound.expect("bind rejecting server");

    let exe = env!("CARGO_BIN_EXE_ocn-node");
    let run = tokio::time::timeout(
        Duration::from_secs(90),
        tokio::process::Command::new(exe)
            .args([
                "--node-id",
                "node-red-client",
                "--listen",
                "127.0.0.1:0",
                "--peer",
                &addr,
                "--token",
                "ocn-secret-2026",
            ])
            .output(),
    )
    .await
    .expect("ocn-node did not finish within 90s")
    .expect("failed to spawn ocn-node");

    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);

    // Guard against a false pass caused by the binary dying before it ever tried
    // to connect (that would be an environment/regression problem, not this defect).
    assert!(
        stdout.contains("Connection attempt"),
        "harness error: ocn-node never reached the retry loop. stdout: {} | stderr: {}",
        tail(&stdout, 8),
        tail(&stderr, 8)
    );

    assert!(
        !run.status.success(),
        "RED B-3: ocn-node exited successfully (exit code {:?}) after every connection \
         attempt failed. stdout: {} | stderr: {}",
        run.status.code(),
        tail(&stdout, 8),
        tail(&stderr, 8)
    );
}
