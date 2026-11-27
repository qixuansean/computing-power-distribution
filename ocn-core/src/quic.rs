use quinn::{Endpoint, Connection, RecvStream, SendStream};
use async_trait::async_trait;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::time::Duration;
use tracing::{info, warn, error};

use crate::identity::NodeIdentity;
use crate::identity::get_pairing_store;
use crate::proto::{FixedHeader, CONTROL_MAX_PAYLOAD_SIZE, read_fixed_frame, generate_secure_nonce_128};
use crate::transport::{Transport, ConnectionHandle};
use crate::auth::{extract_tls_exporter_binding, derive_session_auth_key, compute_auth_proof, PendingChallengeStore};
use crate::messages::ConnectionState;
use crate::task_wire::{read_task_frame, write_task_frame, TaskRequestFrame, TaskResultFrame};

pub struct QuicTransport {
    identity: NodeIdentity,
    expected_peer_fingerprint: Option<String>,
    pairing_token: String,
    node_name: String,
    platform_id: String,
    _listen_port: u16,
    capabilities: Vec<String>,
    challenge_store: Arc<Mutex<PendingChallengeStore>>,
}

impl QuicTransport {
    pub fn new(
        identity: NodeIdentity,
        expected_peer_fingerprint: Option<String>,
        pairing_token: String,
        node_name: String,
        platform_id: String,
        listen_port: u16,
        capabilities: Vec<String>,
    ) -> Self {
        Self {
            identity,
            expected_peer_fingerprint,
            pairing_token,
            node_name,
            platform_id,
            _listen_port: listen_port,
            capabilities,
            challenge_store: Arc::new(Mutex::new(PendingChallengeStore::new(30, 10000, 10))),
        }
    }
}

pub struct QuicConnection {
    pub connection: Connection,
    pub state: ConnectionState,
    pub peer_node_id: Option<String>,
}

#[async_trait]
impl ConnectionHandle for QuicConnection {
    async fn send(&mut self, packet: &crate::proto::OCNPacket) -> anyhow::Result<()> {
        let bytes = packet.serialize()?;
        let len = bytes.len() as u32;
        let (mut send, _recv) = self.connection.open_bi().await?;
        send.write_all(&len.to_be_bytes()).await?;
        send.write_all(&bytes).await?;
        send.finish()?;
        Ok(())
    }

    async fn recv(&mut self) -> anyhow::Result<Option<crate::proto::OCNPacket>> {
        match self.connection.accept_bi().await {
            Ok((_send, mut recv)) => {
                let mut len_buf = [0u8; 4];
                match tokio::time::timeout(Duration::from_secs(10), recv.read_exact(&mut len_buf)).await {
                    Ok(Ok(())) => {
                        let len = u32::from_be_bytes(len_buf) as usize;
                        if len > crate::proto::MAX_PACKET_SIZE {
                            anyhow::bail!("Packet size {} exceeds limit", len);
                        }
                        let mut buf = vec![0u8; len];
                        recv.read_exact(&mut buf).await?;
                        let pkt = crate::proto::OCNPacket::deserialize(&buf)?;
                        Ok(Some(pkt))
                    }
                    _ => Ok(None),
                }
            }
            Err(_) => Ok(None),
        }
    }

    fn remote_addr(&self) -> SocketAddr {
        self.connection.remote_address()
    }

    fn as_quic(&self) -> Option<&QuicConnection> {
        Some(self)
    }
}

/// Client side of a single task exchange.
///
/// The request was written on the send half of a fresh bidirectional stream, which has
/// already been finished. The response is read from the receive half of that same
/// stream, so results can never be confused between concurrent tasks.
pub struct TaskExchange {
    recv: RecvStream,
}

impl TaskExchange {
    /// Read and validate the server's result, bounded by `timeout`.
    pub async fn recv_result_with_timeout(
        &mut self,
        timeout: Duration,
    ) -> anyhow::Result<TaskResultFrame> {
        let result: TaskResultFrame = read_task_frame(&mut self.recv, timeout).await?;
        result.validate()?;
        Ok(result)
    }
}

/// Server side of a single task exchange, obtained from [`QuicConnection::accept_task`].
pub struct TaskStream {
    send: SendStream,
    recv: RecvStream,
}

impl TaskStream {
    /// Decode the task request written by the client.
    ///
    /// The wire fields are intentionally not validated here so the server can echo the
    /// decoded task id in a structured error result before rejecting the request.
    pub async fn recv_request(
        &mut self,
        timeout: Duration,
    ) -> anyhow::Result<TaskRequestFrame> {
        read_task_frame(&mut self.recv, timeout).await
    }

    /// Write the result back on this stream and finish the send half.
    pub async fn send_result(&mut self, result: &TaskResultFrame) -> anyhow::Result<()> {
        write_task_frame(&mut self.send, result).await?;
        self.send.finish()?;
        Ok(())
    }

    /// Abort this stream without consuming further input.
    pub fn reset(&mut self, code: u32) {
        let _ = self.send.reset(code.into());
        let _ = self.recv.stop(code.into());
    }
}

impl QuicConnection {
    /// Client: open one bidirectional stream, write `request`, and return the exchange.
    ///
    /// Uses a fresh stream per task, as required for independent concurrent tasks.
    pub async fn open_task(&self, request: &TaskRequestFrame) -> anyhow::Result<TaskExchange> {
        request.validate()?;
        let (mut send, recv) = self.connection.open_bi().await?;
        write_task_frame(&mut send, request).await?;
        send.finish()?;
        Ok(TaskExchange { recv })
    }

    /// Server: accept the next task stream opened by the authenticated peer.
    ///
    /// Returns `Ok(None)` when `idle_timeout` elapses without a new stream, which lets
    /// callers poll shutdown conditions without treating idleness as an error.
    pub async fn accept_task(&self, idle_timeout: Duration) -> anyhow::Result<Option<TaskStream>> {
        match tokio::time::timeout(idle_timeout, self.connection.accept_bi()).await {
            Ok(Ok((send, recv))) => Ok(Some(TaskStream { send, recv })),
            Ok(Err(e)) => Err(e.into()),
            Err(_) => Ok(None),
        }
    }
}

#[async_trait]
impl Transport for QuicTransport {
    async fn start_server(&self, addr: &str) -> anyhow::Result<mpsc::Receiver<(crate::proto::OCNPacket, Box<dyn ConnectionHandle>)>> {
        let socket_addr: SocketAddr = addr.parse()?;
        let server_config = self.identity.to_server_config(None)?;
        let endpoint = Endpoint::server(server_config, socket_addr)?;
        let (tx, rx) = mpsc::channel(100);

        let identity = self.identity.clone();
        let pairing_token = self.pairing_token.clone();
        let node_name = self.node_name.clone();
        let platform_id = self.platform_id.clone();
        let capabilities = self.capabilities.clone();
        let challenge_store = self.challenge_store.clone();

        tokio::spawn(async move {
            while let Some(incoming) = endpoint.accept().await {
                let identity = identity.clone();
                let pairing_token = pairing_token.clone();
                let node_name = node_name.clone();
                let platform_id = platform_id.clone();
                let capabilities = capabilities.clone();
                let challenge_store = challenge_store.clone();
                let tx = tx.clone();

                tokio::spawn(async move {
                    match incoming.accept() {
                        Ok(conn_fut) => {
                            match conn_fut.await {
                                Ok(connection) => {
                                    info!("Incoming QUIC connection accepted from {}", connection.remote_address());

                                    let mut state;

                                    // Accept bidirectional stream for auth & messaging
                                    if let Ok((_send, mut recv)) = connection.accept_bi().await {
                                        // Read fixed frame with 10s timeout
                                        if let Ok((header, payload)) = read_fixed_frame(&mut recv, CONTROL_MAX_PAYLOAD_SIZE, Duration::from_secs(10)).await {
                                            if header.msg_type == 1 { // Hello
                                                // Parse Hello
                                                let mut offset = 0;
                                                let client_node_id = match crate::proto::encode::read_string(&payload, &mut offset) {
                                                    Ok(s) => s,
                                                    Err(e) => {
                                                        error!("Failed to parse Hello client_node_id: {:?}", e);
                                                        return;
                                                    }
                                                };
                                                let _proto_ver = match crate::proto::encode::read_u16(&payload, &mut offset) {
                                                    Ok(v) => v,
                                                    Err(_) => return,
                                                };
                                                let client_nonce = match crate::proto::encode::read_nonce16(&payload, &mut offset) {
                                                    Ok(n) => n,
                                                    Err(_) => return,
                                                };

                                                state = ConnectionState::WaitAuthResponse;

                                                // Generate Challenge
                                                let server_challenge = generate_nonce_128();
                                                let exporter_binding = match extract_tls_exporter_binding(&connection) {
                                                    Ok(b) => b,
                                                    Err(e) => {
                                                        error!("Failed to extract TLS exporter binding: {:?}", e);
                                                        return;
                                                    }
                                                };

                                                // *** B-1b: Use client's node_id from Hello as server_node_id ***
                                                // The fingerprint is separate and never used as node_id.
                                                // The auth proof uses self.identity.node_id as the reference.
                                                let server_node_id = &identity.node_id;

                                                // Compute expected auth proof
                                                let session_key = derive_session_auth_key(&pairing_token, &exporter_binding);
                                                let expected_proof = compute_auth_proof(&session_key, &server_challenge, &client_nonce, &client_node_id, &server_node_id);

                                                // Store challenge for later verification
                                                {
                                                    let mut store = challenge_store.lock().await;
                                                    if let Err(e) = store.create_challenge(&client_node_id, server_challenge, exporter_binding) {
                                                        error!("Failed to create challenge: {:?}", e);
                                                        return;
                                                    }
                                                }

                                                // Send AuthChallenge
                                                if let Ok(mut send_stream) = connection.open_bi().await.map(|s| s.0) {
                                                    // *** B-1b/B-2: Construct AuthChallenge v2 ***
                                                    // payload_version:1, nonce:16, server_node_id, fingerprint:32, expiry:4
                                                    let mut auth_ch_payload = Vec::new();
                                                    crate::proto::encode::write_bool(&mut auth_ch_payload, true); // payload_version
                                                    auth_ch_payload.extend_from_slice(&server_challenge);
                                                    if let Err(e) = crate::proto::encode::write_string(&mut auth_ch_payload, &identity.node_id) {
                                                        error!("Failed to write server node_id: {:?}", e);
                                                        return;
                                                    }
                                                    let fp_bytes = hex::decode(&identity.fingerprint).unwrap_or_default();
                                                    auth_ch_payload.extend_from_slice(&fp_bytes);
                                                    auth_ch_payload.extend_from_slice(&30u32.to_be_bytes()); // 30s expiry

                                                    let ch_header = FixedHeader::new(2, auth_ch_payload.len() as u32);
                                                    let _ = send_stream.write_all(&ch_header.encode()).await;
                                                    let _ = send_stream.write_all(&auth_ch_payload).await;
                                                    let _ = send_stream.finish();
                                                }

                                                // Wait for AuthResponse (MsgType 3)
                                                if let Ok((_s2, mut r2)) = connection.accept_bi().await {
                                                    if let Ok((h2, p2)) = read_fixed_frame(&mut r2, CONTROL_MAX_PAYLOAD_SIZE, Duration::from_secs(10)).await {
                                                        if h2.msg_type == 3 {
                                                            let mut off2 = 0;
                                                            let _resp_node_id = match crate::proto::encode::read_string(&p2, &mut off2) {
                                                                Ok(s) => s,
                                                                Err(_) => return,
                                                            };
                                                            let mut auth_proof = [0u8; 32];
                                                            if off2 + 32 <= p2.len() {
                                                                auth_proof.copy_from_slice(&p2[off2..off2 + 32]);
                                                            } else {
                                                                return;
                                                            }

                                                            // Verify challenge and auth proof
                                                            let mut store = challenge_store.lock().await;
                                                            let auth_success = store.check_and_consume(&server_challenge, &client_node_id, &exporter_binding).is_ok() && auth_proof == expected_proof;

                                                            // Send AuthResult (MsgType 4)
                                                            if let Ok(mut s_res) = connection.open_bi().await.map(|s| s.0) {
                                                                let mut res_payload = Vec::new();
                                                                crate::proto::encode::write_bool(&mut res_payload, auth_success);
                                                                crate::proto::encode::write_string(&mut res_payload, if auth_success { "Authenticated successfully" } else { "Authentication failed: invalid proof or challenge" }).unwrap();

                                                                let res_header = FixedHeader::new(4, res_payload.len() as u32);
                                                                let _ = s_res.write_all(&res_header.encode()).await;
                                                                let _ = s_res.write_all(&res_payload).await;
                                                                let _ = s_res.finish();
                                                            }

                                                            if auth_success {
                                                                state = ConnectionState::Authenticated;
                                                                info!("Node '{}' authenticated successfully on server", client_node_id);

                                                                // Send NodeInfo (MsgType 5)
                                                                if let Ok(mut s_info) = connection.open_bi().await.map(|s| s.0) {
                                                                    let mut info_payload = Vec::new();
                                                                    crate::proto::encode::write_string(&mut info_payload, &identity.node_id).unwrap();
                                                                    crate::proto::encode::write_string(&mut info_payload, &node_name).unwrap();
                                                                    crate::proto::encode::write_string(&mut info_payload, &platform_id).unwrap();
                                                                    crate::proto::encode::write_u16(&mut info_payload, 1);
                                                                    crate::proto::encode::write_vec_string(&mut info_payload, &capabilities).unwrap();

                                                                    let info_header = FixedHeader::new(5, info_payload.len() as u32);
                                                                    let _ = s_info.write_all(&info_header.encode()).await;
                                                                    let _ = s_info.write_all(&info_payload).await;
                                                                    let _ = s_info.finish();
                                                                }

                                                                state = ConnectionState::Active;

                                                                // Dispatch to connection handler loop
                                                                let quic_conn = QuicConnection {
                                                                    connection,
                                                                    state,
                                                                    peer_node_id: Some(client_node_id),
                                                                };
                                                                let dummy_packet = crate::proto::OCNPacket::new(crate::proto::OCNMsgType::AuthResponse { success: true, message: "Authenticated".into() });
                                                                let _ = tx.send((dummy_packet, Box::new(quic_conn) as Box<dyn ConnectionHandle>)).await;
                                                            } else {
                                                                warn!("Authentication failed for client '{}'", client_node_id);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(e) => error!("Connection accept failed: {:?}", e),
                            }
                        }
                        Err(e) => error!("Incoming connection rejected: {:?}", e),
                    }
                });
            }
        });

        Ok(rx)
    }

    async fn connect(&self, addr: &str) -> anyhow::Result<Box<dyn ConnectionHandle>> {
        let socket_addr: SocketAddr = addr.parse()?;
        let client_config = self.identity.to_client_config(self.expected_peer_fingerprint.clone())?;
        let mut endpoint = Endpoint::client("0.0.0.0:0".parse().unwrap())?;
        endpoint.set_default_client_config(client_config);

        let connecting = endpoint.connect(socket_addr, &self.identity.node_id)?;
        let connection = connecting.await?;
        info!("Connected to server at {}", socket_addr);

        let mut state;

        // 1. Send Hello (MsgType 1)
        let client_nonce = generate_nonce_128();
        let mut hello_payload = Vec::new();
        crate::proto::encode::write_string(&mut hello_payload, &self.identity.node_id)?;
        crate::proto::encode::write_u16(&mut hello_payload, 1);
        crate::proto::encode::write_nonce16(&mut hello_payload, &client_nonce);

        let mut send_stream = connection.open_bi().await?.0;
        let hello_header = FixedHeader::new(1, hello_payload.len() as u32);
        send_stream.write_all(&hello_header.encode()).await?;
        send_stream.write_all(&hello_payload).await?;
        send_stream.finish()?;

        state = ConnectionState::WaitAuthResponse;

        // 2. Receive AuthChallenge (MsgType 2)
        let (_s_in, mut r_in) = connection.accept_bi().await?;
        let (ch_header, ch_payload) = read_fixed_frame(&mut r_in, CONTROL_MAX_PAYLOAD_SIZE, Duration::from_secs(10)).await?;
        if ch_header.msg_type != 2 {
            anyhow::bail!("Expected AuthChallenge (2), got msg_type {}", ch_header.msg_type);
        }

        let mut ch_offset = 0;
        let _payload_version = crate::proto::encode::read_bool(&ch_payload, &mut ch_offset)?;
        let server_challenge = crate::proto::encode::read_nonce16(&ch_payload, &mut ch_offset)?;
        let server_node_id = crate::proto::encode::read_string(&ch_payload, &mut ch_offset)?;
        // fingerprint: 32 raw bytes (no length prefix)
        if ch_offset + 32 > ch_payload.len() {
            anyhow::bail!("AuthChallenge payload too short for fingerprint");
        }
        let server_fp_bytes = ch_payload[ch_offset..ch_offset + 32].to_vec();
        ch_offset += 32;
        let _expires = crate::proto::encode::read_u32(&ch_payload, &mut ch_offset)?;

        // *** B-2: Verify server identity against pairing store ***
        // The server_node_id and fingerprint from AuthChallenge v2 must match our trusted pairing record.
        // TLS expected_peer_fingerprint is only used for cert pinning, NOT for node_id.
        let server_fingerprint = hex::encode(&server_fp_bytes);
        let pairing_check = crate::identity::get_pairing_store().lock().unwrap().check_pairing(&server_node_id, &server_fingerprint);
        match pairing_check {
            Some(true) => {
                // Paired: server_node_id and fingerprint match trusted record
            }
            Some(false) => {
                anyhow::bail!("Authentication rejected: server node_id/fingerprint mismatch in pairing store");
            }
            None => {
                anyhow::bail!("Authentication rejected: unpaired server node_id/fingerprint; no pairing record");
            }
        }

        // Extract exporter binding and compute AuthProof
        let exporter_binding = extract_tls_exporter_binding(&connection)?;
        let session_key = derive_session_auth_key(&self.pairing_token, &exporter_binding);

        // Use the server_node_id from AuthChallenge (verified above) for proof computation.
        let auth_proof = compute_auth_proof(&session_key, &server_challenge, &client_nonce, &self.identity.node_id, &server_node_id);

        // 3. Send AuthResponse (MsgType 3)
        let mut resp_payload = Vec::new();
        crate::proto::encode::write_string(&mut resp_payload, &self.identity.node_id)?;
        resp_payload.extend_from_slice(&auth_proof);

        let mut send_resp = connection.open_bi().await?.0;
        let resp_header = FixedHeader::new(3, resp_payload.len() as u32);
        send_resp.write_all(&resp_header.encode()).await?;
        send_resp.write_all(&resp_payload).await?;
        send_resp.finish()?;

        // 4. Receive AuthResult (MsgType 4)
        let (_s_res, mut r_res) = connection.accept_bi().await?;
        let (res_header, res_payload) = read_fixed_frame(&mut r_res, CONTROL_MAX_PAYLOAD_SIZE, Duration::from_secs(10)).await?;
        if res_header.msg_type != 4 {
            anyhow::bail!("Expected AuthResult (4), got {}", res_header.msg_type);
        }

        let mut res_offset = 0;
        let success = crate::proto::encode::read_bool(&res_payload, &mut res_offset)?;
        let msg = crate::proto::encode::read_string(&res_payload, &mut res_offset)?;

        if !success {
            anyhow::bail!("Authentication rejected by server: {}", msg);
        }

        state = ConnectionState::Authenticated;
        info!("Client authenticated successfully with server");

        // 5. Receive NodeInfo (MsgType 5)
        let (_s_info, mut r_info) = connection.accept_bi().await?;
        let (info_header, _info_payload) = read_fixed_frame(&mut r_info, CONTROL_MAX_PAYLOAD_SIZE, Duration::from_secs(10)).await?;
        if info_header.msg_type == 5 {
            state = ConnectionState::Active;
            info!("Received NodeInfo from server, connection active");
        }

        Ok(Box::new(QuicConnection {
            connection,
            state,
            peer_node_id: Some(server_node_id.to_string()),
        }) as Box<dyn ConnectionHandle>)
    }
}

fn generate_nonce_128() -> [u8; 16] {
    generate_secure_nonce_128()
}