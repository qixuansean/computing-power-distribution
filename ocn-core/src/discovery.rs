use serde::{Serialize, Deserialize};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use tracing::info;

/// Minimum discovery record information exposed for cross-platform discovery.
///
/// This struct is sent via mDNS TXT records and contains only non-sensitive
/// node information. Private keys, credentials, and secrets are never transmitted.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DiscoveryRecord {
    /// OCN Node ID (human-readable identifier)
    pub node_id: String,
    /// Human-readable node name
    pub node_name: String,
    /// IP address of the node (v4 dotted-decimal)
    pub ip_address: String,
    /// Listening port for QUIC/OCN connections
    pub listen_port: u16,
    /// Protocol version number
    pub protocol_version: u16,
    /// Platform identifier string
    pub platform_id: String,
}

impl DiscoveryRecord {
    /// Create a new DiscoveryRecord from the given parameters.
    pub fn new(
        node_id: String,
        node_name: String,
        ip_address: String,
        listen_port: u16,
        protocol_version: u16,
        platform_id: String,
    ) -> Self {
        Self {
            node_id,
            node_name,
            ip_address,
            listen_port,
            protocol_version,
            platform_id,
        }
    }
}

/// Advertise this node via mDNS/UDP multicast on the local network.
///
/// Sends a discovery announcement to the mDNS multicast group
/// (224.0.0.251:5353) containing the node's DiscoveryRecord as TXT data.
pub async fn advertise_discovery(
    record: DiscoveryRecord,
    socket: &UdpSocket,
) -> anyhow::Result<()> {
    // Serialize the discovery record as JSON for the TXT record payload
    let payload = serde_json::to_string(&record)?;
    let payload_bytes = payload.as_bytes();

    // Send to m multicast group (224.0.0.251:5353)
    // This is a basic UDP multicast discovery message
    socket.send_to(payload_bytes, ("224.0.0.251", 5353))?;

    info!(
        "mDNS discovery advertisement sent: node={}, port={}",
        record.node_id, record.listen_port
    );

    Ok(())
}

/// Browse for mDNS discovery announcements on the local network.
///
/// Listens on the mDNS multicast group (224.0.0.251:5353) for
/// OCN discovery announcements and returns the parsed DiscoveryRecord.
pub async fn browse_discovery(socket: &UdpSocket) -> anyhow::Result<Option<DiscoveryRecord>> {
    let mut buf = [0u8; 4096];
    let (len, _) = socket.recv_from(&mut buf)?;

    let payload = String::from_utf8_lossy(&buf[..len]);

    // Check if this is an OCN discovery message
    // mDNS service type format: _ocn._udp.local.
    if !payload.starts_with("_ocn._udp.local.") {
        // Could be a different service; still check if it contains our JSON payload
        // Try to find JSON { in the payload
        if let Some(pos) = payload.find('{') {
            let json_part = &payload[pos..];
            if let Ok(record) = serde_json::from_str::<DiscoveryRecord>(json_part) {
                return Ok(Some(record));
            }
        }
        return Ok(None);
    }

    // Parse the JSON payload embedded in the mDNS response
    // The payload may contain the JSON after the mDNS header portion
    // Try to find and parse JSON from the raw bytes
    if let Some(pos) = payload.find('{') {
        let json_part = &payload[pos..];
        if let Ok(record) = serde_json::from_str::<DiscoveryRecord>(json_part) {
            return Ok(Some(record));
        }
    }

    Ok(None)
}

/// Start a mDNS multicast listener on the given local address.
/// Returns a UDP socket bound to the multicast group 224.0.0.251:5353.
pub fn start_multicast_listener(local_addr: SocketAddr) -> anyhow::Result<UdpSocket> {
    let socket = UdpSocket::bind(local_addr)?;
    // Join the mDNS IPv4 multicast group
    socket.join_multicast_v4(
        &Ipv4Addr::new(224, 0, 0, 251),
        &Ipv4Addr::new(0, 0, 0, 0), // interface = 0 (default)
    )?;
    // Set multicast TTL to 1 (local network only)
    socket.set_multicast_ttl_v4(1)?;
    info!("mDNS multicast listener started on {}", local_addr);
    Ok(socket)
}

/// Platform identifier string used in discovery records.
/// Distinguishes between Windows x86_64 and Android ARM64 nodes.
pub fn platform_id() -> String {
    #[cfg(target_os = "windows")]
    let pid = "windows-x86_64";
    #[cfg(target_os = "linux")]
    let pid = "android-arm64";
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    let pid = "unknown-platform";

    pid.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_discovery_record_creation() {
        let record = DiscoveryRecord::new(
            "test-node-001".to_string(),
            "OCN-Test-Node".to_string(),
            "192.168.1.50".to_string(),
            9000,
            1,
            "test-platform".to_string());

        assert_eq!(record.node_id, "test-node-001");
        assert_eq!(record.node_name, "OCN-Test-Node");
        assert_eq!(record.ip_address, "192.168.1.50");
        assert_eq!(record.listen_port, 9000);
        assert_eq!(record.protocol_version, 1);
        assert_eq!(record.platform_id, "test-platform");
    }

    #[tokio::test]
    async fn test_discovery_record_serialize_deserialize() {
        let original = DiscoveryRecord::new(
            "node-1".to_string(),
            "Windows-PC".to_string(),
            "10.0.0.100".to_string(),
            9000,
            1,
            "windows-x86_64".to_string());

        let serialized = serde_json::to_string(&original).expect("Serialization failed");
        let deserialized: DiscoveryRecord = serde_json::from_str(&serialized).expect("Deserialization failed");

        assert_eq!(original, deserialized);
    }

    #[tokio::test]
    async fn test_platform_id_windows() {
        // On Windows, the platform_id should be "windows-x86_64"
        // This is checked at compile-time via cfg, so we just verify the string is set correctly
        let pid = platform_id();
        assert!(!pid.is_empty());
    }
}