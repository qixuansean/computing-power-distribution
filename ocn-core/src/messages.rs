use serde::{Serialize, Deserialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    TlsHandshake,
    WaitHello,
    WaitAuthResponse,
    Authenticated,
    Active,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HelloPayload {
    pub client_node_id: String,
    pub protocol_version: u16,
    pub client_nonce: [u8; 16],
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthChallengePayload {
    pub server_challenge_nonce: [u8; 16],
    pub expires_in_secs: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthResponsePayload {
    pub client_node_id: String,
    pub auth_proof: [u8; 32],
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthResultPayload {
    pub success: bool,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeInfoPayload {
    pub node_id: String,
    pub node_name: String,
    pub platform_id: String,
    pub protocol_version: u16,
    pub capabilities: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PingPayload {
    pub timestamp_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PongPayload {
    pub timestamp_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DisconnectPayload {
    pub reason: String,
}
