use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OCNConfig {
    pub node_id: String,
    pub listen_addr: String,
    pub peer_addr: Option<String>,
    pub auth_token: String,
}

impl Default for OCNConfig {
    fn default() -> Self {
        Self {
            node_id: "node-default".to_string(),
            listen_addr: "127.0.0.1:9000".to_string(),
            peer_addr: None,
            auth_token: "ocn-secret-token-2026".to_string(),
        }
    }
}
