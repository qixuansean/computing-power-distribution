pub mod config;
pub mod identity;
pub mod transport;
pub mod quic;
pub mod proto;
pub mod metrics;
pub mod discovery;
pub mod auth;
pub mod messages;

#[cfg(feature = "android-jni")]
pub mod jni_api;

#[cfg(test)]
mod tests;

pub use config::OCNConfig;
pub use identity::NodeIdentity;
pub use transport::{Transport, ConnectionHandle};
pub use quic::{QuicTransport, QuicConnection};
pub use proto::{OCNPacket, OCNMsgType, MAX_PACKET_SIZE, generate_secure_nonce, generate_secure_nonce_128, FixedHeader, HEADER_SIZE, CONTROL_MAX_PAYLOAD_SIZE, DATA_MAX_PAYLOAD_SIZE, read_fixed_frame};
pub use metrics::NodeMetrics;
pub use discovery::{DiscoveryRecord, platform_id, start_multicast_listener, advertise_discovery, browse_discovery};
pub use auth::{PendingChallengeStore, PendingChallenge, ReplayCache, ReplayKey, extract_tls_exporter_binding, derive_session_auth_key, compute_auth_proof};
pub use messages::{ConnectionState, HelloPayload, AuthChallengePayload, AuthResponsePayload, AuthResultPayload, NodeInfoPayload, PingPayload, PongPayload, DisconnectPayload};
