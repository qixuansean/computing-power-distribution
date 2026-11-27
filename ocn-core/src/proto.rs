use ring::rand::{SecureRandom, SystemRandom};
use tokio::io::AsyncReadExt;
use tokio::time::{Instant, Duration, timeout_at};
use serde::{Serialize, Deserialize};

pub const PROTOCOL_MAGIC: [u8; 4] = [0x4F, 0x43, 0x4E, 0x31]; // "OCN1"
pub const PROTOCOL_VERSION: u16 = 1;
pub const HEADER_SIZE: usize = 12;

pub const CONTROL_MAX_PAYLOAD_SIZE: u32 = 64 * 1024;      // 64 KB limit
pub const DATA_MAX_PAYLOAD_SIZE: u32 = 2 * 1024 * 1024;   // 2 MB limit
pub const MAX_PACKET_SIZE: usize = 10 * 1024 * 1024;       // 10 MB legacy limit

pub fn generate_secure_nonce_128() -> [u8; 16] {
    let rng = SystemRandom::new();
    let mut buf = [0u8; 16];
    rng.fill(&mut buf).expect("Failed to generate cryptographically secure 128-bit nonce");
    buf
}

pub fn generate_secure_nonce() -> u64 {
    let rng = SystemRandom::new();
    let mut buf = [0u8; 8];
    rng.fill(&mut buf).expect("Failed to generate cryptographically secure 64-bit nonce");
    u64::from_be_bytes(buf)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixedHeader {
    pub magic: [u8; 4],
    pub version: u16,
    pub msg_type: u16,
    pub payload_size: u32,
}

impl FixedHeader {
    pub fn new(msg_type: u16, payload_size: u32) -> Self {
        Self {
            magic: PROTOCOL_MAGIC,
            version: PROTOCOL_VERSION,
            msg_type,
            payload_size,
        }
    }

    pub fn encode(&self) -> [u8; 12] {
        let mut buf = [0u8; 12];
        buf[0..4].copy_from_slice(&self.magic);
        buf[4..6].copy_from_slice(&self.version.to_be_bytes());
        buf[6..8].copy_from_slice(&self.msg_type.to_be_bytes());
        buf[8..12].copy_from_slice(&self.payload_size.to_be_bytes());
        buf
    }

    pub fn decode(buf: &[u8; 12]) -> anyhow::Result<Self> {
        let magic = [buf[0], buf[1], buf[2], buf[3]];
        if magic != PROTOCOL_MAGIC {
            anyhow::bail!("Invalid protocol magic header: {:?}", magic);
        }
        let version = u16::from_be_bytes([buf[4], buf[5]]);
        if version != PROTOCOL_VERSION {
            anyhow::bail!("Unsupported protocol version: {}", version);
        }
        let msg_type = u16::from_be_bytes([buf[6], buf[7]]);
        let payload_size = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]);
        Ok(Self {
            magic,
            version,
            msg_type,
            payload_size,
        })
    }
}

pub mod encode {
    pub fn write_bool(buf: &mut Vec<u8>, val: bool) {
        buf.push(if val { 0x01 } else { 0x00 });
    }

    pub fn read_bool(slice: &[u8], offset: &mut usize) -> anyhow::Result<bool> {
        if *offset >= slice.len() {
            anyhow::bail!("EOF reading bool");
        }
        let b = slice[*offset];
        *offset += 1;
        match b {
            0x00 => Ok(false),
            0x01 => Ok(true),
            _ => anyhow::bail!("Invalid bool byte 0x{:02X}", b),
        }
    }

    pub fn write_u16(buf: &mut Vec<u8>, val: u16) {
        buf.extend_from_slice(&val.to_be_bytes());
    }

    pub fn read_u16(slice: &[u8], offset: &mut usize) -> anyhow::Result<u16> {
        if *offset + 2 > slice.len() {
            anyhow::bail!("EOF reading u16");
        }
        let val = u16::from_be_bytes([slice[*offset], slice[*offset + 1]]);
        *offset += 2;
        Ok(val)
    }

    pub fn write_u32(buf: &mut Vec<u8>, val: u32) {
        buf.extend_from_slice(&val.to_be_bytes());
    }

    pub fn read_u32(slice: &[u8], offset: &mut usize) -> anyhow::Result<u32> {
        if *offset + 4 > slice.len() {
            anyhow::bail!("EOF reading u32");
        }
        let val = u32::from_be_bytes([slice[*offset], slice[*offset + 1], slice[*offset + 2], slice[*offset + 3]]);
        *offset += 4;
        Ok(val)
    }

    pub fn write_nonce16(buf: &mut Vec<u8>, val: &[u8; 16]) {
        buf.extend_from_slice(val);
    }

    pub fn read_nonce16(slice: &[u8], offset: &mut usize) -> anyhow::Result<[u8; 16]> {
        if *offset + 16 > slice.len() {
            anyhow::bail!("EOF reading nonce16");
        }
        let mut buf = [0u8; 16];
        buf.copy_from_slice(&slice[*offset..*offset + 16]);
        *offset += 16;
        Ok(buf)
    }

    pub fn write_string(buf: &mut Vec<u8>, val: &str) -> anyhow::Result<()> {
        let bytes = val.as_bytes();
        if bytes.len() > 16384 {
            anyhow::bail!("string too long");
        }
        write_u32(buf, bytes.len() as u32);
        buf.extend_from_slice(bytes);
        Ok(())
    }

    pub fn read_string(slice: &[u8], offset: &mut usize) -> anyhow::Result<String> {
        let len = read_u32(slice, offset)? as usize;
        if len > 16384 {
            anyhow::bail!("string too long");
        }
        if *offset + len > slice.len() {
            anyhow::bail!("EOF reading string");
        }
        let s = String::from_utf8(slice[*offset..*offset + len].to_vec())?;
        *offset += len;
        Ok(s)
    }

    pub fn write_vec_string(buf: &mut Vec<u8>, vals: &[String]) -> anyhow::Result<()> {
        write_u32(buf, vals.len() as u32);
        for v in vals {
            write_string(buf, v)?;
        }
        Ok(())
    }

    pub fn read_vec_string(slice: &[u8], offset: &mut usize) -> anyhow::Result<Vec<String>> {
        let len = read_u32(slice, offset)? as usize;
        let mut res = Vec::with_capacity(len);
        for _ in 0..len {
            res.push(read_string(slice, offset)?);
        }
        Ok(res)
    }

    pub fn read_vec_bytes(slice: &[u8], offset: &mut usize, max_len: usize) -> anyhow::Result<Vec<u8>> {
        let len = read_u32(slice, offset)? as usize;
        if len > max_len {
            anyhow::bail!("Vec bytes length {} exceeds max limit {}", len, max_len);
        }
        if *offset + len > slice.len() {
            anyhow::bail!("EOF reading Vec<u8> payload of length {}", len);
        }
        let res = slice[*offset..*offset + len].to_vec();
        *offset += len;
        Ok(res)
    }
}

pub async fn read_fixed_frame<R: AsyncReadExt + Unpin>(
    stream: &mut R,
    max_payload_size: u32,
    total_frame_timeout: Duration,
) -> anyhow::Result<(FixedHeader, Vec<u8>)> {
    let deadline = Instant::now() + total_frame_timeout;

    timeout_at(deadline, async {
        let mut header_buf = [0u8; HEADER_SIZE];
        stream.read_exact(&mut header_buf).await?;
        let header = FixedHeader::decode(&header_buf)?;

        if header.payload_size > max_payload_size {
            anyhow::bail!("Payload size {} exceeds limit {}", header.payload_size, max_payload_size);
        }

        let mut payload_buf = vec![0u8; header.payload_size as usize];
        if header.payload_size > 0 {
            stream.read_exact(&mut payload_buf).await?;
        }

        Ok((header, payload_buf))
    }).await?
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OCNHeader {
    pub magic: [u8; 4],
    pub version: u16,
}

impl Default for OCNHeader {
    fn default() -> Self {
        Self {
            magic: PROTOCOL_MAGIC,
            version: PROTOCOL_VERSION,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum OCNMsgType {
    AuthRequest { node_id: String, token: String, nonce: u64 },
    AuthResponse { success: bool, message: String },
    Echo { payload: String },
    HashTest { data: Vec<u8>, expected_hash: String },
    HashTestResult { success: bool, computed_hash: String },
    Ping { timestamp: u64 },
    Pong { timestamp: u64 },
    ThroughputStart { chunk_size: usize, total_chunks: usize },
    ThroughputChunk { chunk_id: usize, data: Vec<u8> },
    ThroughputAck { chunk_id: usize },
    TaskRequest { task_id: String, payload: Vec<u8>, input_hash: [u8; 32], timeout_ms: u64 },
    TaskResult { task_id: String, result: Vec<u8>, result_hash: [u8; 32], success: bool, error_msg: Option<String> },
    Disconnect,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OCNPacket {
    pub header: OCNHeader,
    pub msg_type: OCNMsgType,
}

impl OCNPacket {
    pub fn new(msg_type: OCNMsgType) -> Self {
        Self {
            header: OCNHeader::default(),
            msg_type,
        }
    }

    pub fn serialize(&self) -> anyhow::Result<Vec<u8>> {
        let bytes = bincode::serialize(self)?;
        if bytes.len() > MAX_PACKET_SIZE {
            anyhow::bail!("Packet size {} exceeds maximum allowed limit {}", bytes.len(), MAX_PACKET_SIZE);
        }
        Ok(bytes)
    }

    pub fn deserialize(bytes: &[u8]) -> anyhow::Result<Self> {
        if bytes.len() > MAX_PACKET_SIZE {
            anyhow::bail!("Received payload size {} exceeds maximum allowed limit {}", bytes.len(), MAX_PACKET_SIZE);
        }
        let packet: Self = bincode::deserialize(bytes)?;
        if packet.header.magic != PROTOCOL_MAGIC {
            anyhow::bail!("Invalid protocol magic header: {:?}", packet.header.magic);
        }
        if packet.header.version != PROTOCOL_VERSION {
            anyhow::bail!("Unsupported protocol version: {}", packet.header.version);
        }
        Ok(packet)
    }
}