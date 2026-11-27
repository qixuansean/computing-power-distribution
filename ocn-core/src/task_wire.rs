//! Explicit wire protocol for compute task exchange over QUIC bidirectional streams.
//!
//! Each task uses exactly one QUIC bidirectional stream. The client writes a single
//! length-prefixed [`TaskRequestFrame`] on the send half, finishes that half, and then
//! reads a single length-prefixed [`TaskResultFrame`] from the receive half of the very
//! same stream. The server reads the request, executes it, and writes the result back on
//! the stream's send half. Because request and response share one stream, concurrent
//! tasks can never cross their results.
//!
//! Framing:
//! ```text
//! +----------------+--------------------------------------+
//! | body length u32| bincode(TaskRequestFrame/ResultFrame)|
//! | big endian     |                                      |
//! +----------------+--------------------------------------+
//! ```
//!
//! Both frame types carry an explicit wire version and message id so that a peer can
//! reject unknown protocols deterministically instead of guessing from field offsets.

use std::fmt;
use std::time::Duration;

use quinn::{RecvStream, SendStream};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

/// Version of the task exchange wire format.
pub const TASK_WIRE_VERSION: u16 = 1;

/// Explicit message id for a task request.
pub const TASK_MSG_REQUEST: u16 = 1;

/// Explicit message id for a task result (including error results).
pub const TASK_MSG_RESULT: u16 = 2;

/// Maximum accepted task identifier length.
pub const TASK_MAX_ID_LEN: usize = 128;

/// Maximum accepted task payload / result size.
pub const TASK_MAX_PAYLOAD_SIZE: usize = 10 * 1024 * 1024;

/// Maximum accepted encoded frame body. Payload plus bookkeeping fields.
pub const TASK_MAX_FRAME_SIZE: usize = TASK_MAX_PAYLOAD_SIZE + 64 * 1024;

/// Minimum timeout accepted by a request frame (mirrors the compute task DTO).
pub const TASK_MIN_TIMEOUT_MS: u64 = 1_000;

/// Maximum timeout accepted by a request frame (mirrors the compute task DTO).
pub const TASK_MAX_TIMEOUT_MS: u64 = 3_600_000;

/// Machine-readable error codes carried by failed [`TaskResultFrame`]s.
pub mod error_code {
    /// The frame's wire version is not supported by this peer.
    pub const BAD_VERSION: u32 = 1;
    /// The frame's message id does not match the expected direction.
    pub const BAD_MESSAGE_ID: u32 = 2;
    /// The task id is empty or exceeds [`super::TASK_MAX_ID_LEN`].
    pub const BAD_TASK_ID: u32 = 3;
    /// The payload or encoded frame exceeds the size limit.
    pub const PAYLOAD_TOO_LARGE: u32 = 4;
    /// The task descriptor failed validation (for example an input hash mismatch).
    pub const INVALID_TASK: u32 = 5;
    /// Execution did not finish within the requested timeout.
    pub const TIMEOUT: u32 = 6;
    /// The frame could not be decoded or the stream was interrupted.
    pub const BAD_FRAME: u32 = 7;
    /// The server hit an internal error while executing the task.
    pub const INTERNAL: u32 = 8;
    /// The task type is not supported by the server.
    pub const UNSUPPORTED_TASK: u32 = 9;
    /// An identical task is already being executed.
    pub const ALREADY_RUNNING: u32 = 10;
}

/// Wire-level validation failure for task frames.
#[derive(Debug)]
pub enum TaskWireError {
    /// Unsupported wire version.
    BadVersion(u16),
    /// Message id does not match the expected direction.
    BadMessageId { got: u16, expected: u16 },
    /// Task id is empty or too long.
    BadTaskId,
    /// Payload or frame exceeds the size limit.
    PayloadTooLarge(usize),
    /// Timeout is outside the accepted range.
    BadTimeout(u64),
    /// Result hash does not match the result bytes.
    ResultHashMismatch,
    /// Result task id does not match the request.
    TaskIdMismatch { got: String, expected: String },
}

impl fmt::Display for TaskWireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadVersion(version) => {
                write!(f, "unsupported task wire version {version}, expected {TASK_WIRE_VERSION}")
            }
            Self::BadMessageId { got, expected } => {
                write!(f, "unexpected task message id {got}, expected {expected}")
            }
            Self::BadTaskId => write!(f, "task id must be between 1 and {TASK_MAX_ID_LEN} bytes"),
            Self::PayloadTooLarge(size) => write!(
                f,
                "task payload of {size} bytes exceeds limit of {TASK_MAX_PAYLOAD_SIZE} bytes"
            ),
            Self::BadTimeout(timeout) => write!(
                f,
                "task timeout {timeout} ms is outside [{TASK_MIN_TIMEOUT_MS}, {TASK_MAX_TIMEOUT_MS}] ms"
            ),
            Self::ResultHashMismatch => write!(f, "task result hash does not match the result bytes"),
            Self::TaskIdMismatch { got, expected } => {
                write!(f, "result frame is for task '{got}', expected '{expected}'")
            }
        }
    }
}

impl std::error::Error for TaskWireError {}

impl TaskWireError {
    /// Map a wire validation failure to the stable error code sent to the peer.
    pub fn code(&self) -> u32 {
        match self {
            Self::BadVersion(_) => error_code::BAD_VERSION,
            Self::BadMessageId { .. } => error_code::BAD_MESSAGE_ID,
            Self::BadTaskId => error_code::BAD_TASK_ID,
            Self::PayloadTooLarge(_) => error_code::PAYLOAD_TOO_LARGE,
            Self::BadTimeout(_) => error_code::INVALID_TASK,
            Self::ResultHashMismatch => error_code::INTERNAL,
            Self::TaskIdMismatch { .. } => error_code::BAD_FRAME,
        }
    }
}

/// Request written by a client on a fresh QUIC bidirectional stream.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TaskRequestFrame {
    /// Wire protocol version, always [`TASK_WIRE_VERSION`] for this implementation.
    pub wire_version: u16,
    /// Message id, always [`TASK_MSG_REQUEST`].
    pub msg_id: u16,
    /// Caller supplied task id, stable across retries for deduplication.
    pub task_id: String,
    /// SHA-256 of `payload`, supplied by the client and re-validated by the server.
    pub input_hash: [u8; 32],
    /// Maximum time the caller is willing to wait for a result.
    pub timeout_ms: u64,
    /// Opaque task payload.
    pub payload: Vec<u8>,
}

impl TaskRequestFrame {
    /// Build a request, computing `input_hash` locally.
    pub fn new(task_id: impl Into<String>, payload: Vec<u8>, timeout_ms: u64) -> Self {
        let input_hash = sha256(&payload);
        Self {
            wire_version: TASK_WIRE_VERSION,
            msg_id: TASK_MSG_REQUEST,
            task_id: task_id.into(),
            input_hash,
            timeout_ms,
            payload,
        }
    }

    /// Validate all wire-level fields without touching task semantics.
    pub fn validate(&self) -> Result<(), TaskWireError> {
        if self.wire_version != TASK_WIRE_VERSION {
            return Err(TaskWireError::BadVersion(self.wire_version));
        }
        if self.msg_id != TASK_MSG_REQUEST {
            return Err(TaskWireError::BadMessageId {
                got: self.msg_id,
                expected: TASK_MSG_REQUEST,
            });
        }
        if self.task_id.is_empty() || self.task_id.len() > TASK_MAX_ID_LEN {
            return Err(TaskWireError::BadTaskId);
        }
        if self.payload.len() > TASK_MAX_PAYLOAD_SIZE {
            return Err(TaskWireError::PayloadTooLarge(self.payload.len()));
        }
        if !(TASK_MIN_TIMEOUT_MS..=TASK_MAX_TIMEOUT_MS).contains(&self.timeout_ms) {
            return Err(TaskWireError::BadTimeout(self.timeout_ms));
        }
        Ok(())
    }

    /// Check the caller supplied input hash against the payload.
    pub fn verify_input_hash(&self) -> bool {
        sha256(&self.payload) == self.input_hash
    }
}

/// Result written by the server on the send half of the request stream.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TaskResultFrame {
    /// Wire protocol version, always [`TASK_WIRE_VERSION`].
    pub wire_version: u16,
    /// Message id, always [`TASK_MSG_RESULT`].
    pub msg_id: u16,
    /// Task id echoed from the request.
    pub task_id: String,
    /// Node that executed (or rejected) the task.
    pub node_id: String,
    /// True only when execution completed and the result is valid.
    pub success: bool,
    /// Result bytes. Empty for failures.
    pub result: Vec<u8>,
    /// SHA-256 of `result`. All zeroes for failures.
    pub result_hash: [u8; 32],
    /// Server measured execution time in milliseconds.
    pub execution_time_ms: u64,
    /// True when this result was replayed from the deduplication cache.
    pub duplicate: bool,
    /// Stable machine-readable error code for failures.
    pub error_code: Option<u32>,
    /// Human readable error detail for failures.
    pub error_msg: Option<String>,
}

impl TaskResultFrame {
    /// Build a successful result frame.
    pub fn success(
        task_id: impl Into<String>,
        node_id: impl Into<String>,
        result: Vec<u8>,
        result_hash: [u8; 32],
        execution_time_ms: u64,
        duplicate: bool,
    ) -> Self {
        Self {
            wire_version: TASK_WIRE_VERSION,
            msg_id: TASK_MSG_RESULT,
            task_id: task_id.into(),
            node_id: node_id.into(),
            success: true,
            result,
            result_hash,
            execution_time_ms,
            duplicate,
            error_code: None,
            error_msg: None,
        }
    }

    /// Build a failed result frame.
    pub fn failure(
        task_id: impl Into<String>,
        node_id: impl Into<String>,
        code: u32,
        message: impl Into<String>,
    ) -> Self {
        Self {
            wire_version: TASK_WIRE_VERSION,
            msg_id: TASK_MSG_RESULT,
            task_id: task_id.into(),
            node_id: node_id.into(),
            success: false,
            result: Vec::new(),
            result_hash: [0u8; 32],
            execution_time_ms: 0,
            duplicate: false,
            error_code: Some(code),
            error_msg: Some(message.into()),
        }
    }

    /// Validate wire-level fields and, for successful results, the result hash.
    pub fn validate(&self) -> Result<(), TaskWireError> {
        if self.wire_version != TASK_WIRE_VERSION {
            return Err(TaskWireError::BadVersion(self.wire_version));
        }
        if self.msg_id != TASK_MSG_RESULT {
            return Err(TaskWireError::BadMessageId {
                got: self.msg_id,
                expected: TASK_MSG_RESULT,
            });
        }
        if self.task_id.is_empty() || self.task_id.len() > TASK_MAX_ID_LEN {
            return Err(TaskWireError::BadTaskId);
        }
        if self.result.len() > TASK_MAX_PAYLOAD_SIZE {
            return Err(TaskWireError::PayloadTooLarge(self.result.len()));
        }
        if self.success && !self.verify_result_hash() {
            return Err(TaskWireError::ResultHashMismatch);
        }
        Ok(())
    }

    /// Independently check that `result_hash` is the SHA-256 of `result`.
    pub fn verify_result_hash(&self) -> bool {
        sha256(&self.result) == self.result_hash
    }

    /// Verify that this result answers the given request and is internally consistent.
    pub fn verify_for_request(&self, request: &TaskRequestFrame) -> Result<(), TaskWireError> {
        self.validate()?;
        if self.task_id != request.task_id {
            return Err(TaskWireError::TaskIdMismatch {
                got: self.task_id.clone(),
                expected: request.task_id.clone(),
            });
        }
        Ok(())
    }
}

/// Write one length-prefixed frame. The caller decides when to finish the stream.
pub async fn write_task_frame<T: Serialize>(send: &mut SendStream, frame: &T) -> anyhow::Result<()> {
    let body = bincode::serialize(frame).map_err(|e| anyhow::anyhow!("encode task frame: {e}"))?;
    if body.len() > TASK_MAX_FRAME_SIZE {
        anyhow::bail!(
            "encoded task frame is {} bytes, exceeding limit {}",
            body.len(),
            TASK_MAX_FRAME_SIZE
        );
    }
    send.write_all(&(body.len() as u32).to_be_bytes()).await?;
    send.write_all(&body).await?;
    send.flush().await?;
    Ok(())
}

/// Read exactly one length-prefixed frame, bounded by `timeout`.
pub async fn read_task_frame<T: DeserializeOwned>(
    recv: &mut RecvStream,
    timeout: Duration,
) -> anyhow::Result<T> {
    let body = tokio::time::timeout(timeout, async {
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf)
            .await
            .map_err(|e| anyhow::anyhow!("read task frame length: {e}"))?;
        let len = u32::from_be_bytes(len_buf) as usize;
        if len == 0 {
            anyhow::bail!("task frame length must not be zero");
        }
        if len > TASK_MAX_FRAME_SIZE {
            anyhow::bail!(
                "task frame length {} exceeds limit {}",
                len,
                TASK_MAX_FRAME_SIZE
            );
        }
        let mut body = vec![0u8; len];
        recv.read_exact(&mut body)
            .await
            .map_err(|e| anyhow::anyhow!("read task frame body: {e}"))?;
        Ok::<Vec<u8>, anyhow::Error>(body)
    })
    .await
    .map_err(|_| anyhow::anyhow!("timed out after {:?} waiting for task frame", timeout))??;

    bincode::deserialize(&body).map_err(|e| anyhow::anyhow!("decode task frame: {e}"))
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_ref());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_roundtrip_preserves_fields() {
        let req = TaskRequestFrame::new("task-1", b"hello".to_vec(), 5_000);
        assert!(req.validate().is_ok());
        assert!(req.verify_input_hash());
        let encoded = bincode::serialize(&req).unwrap();
        let decoded: TaskRequestFrame = bincode::deserialize(&encoded).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn request_rejects_bad_version() {
        let mut req = TaskRequestFrame::new("task-1", b"hello".to_vec(), 5_000);
        req.wire_version = 99;
        assert!(matches!(
            req.validate(),
            Err(TaskWireError::BadVersion(99))
        ));
        assert_eq!(req.validate().unwrap_err().code(), error_code::BAD_VERSION);
    }

    #[test]
    fn result_detects_hash_mismatch() {
        let mut res = TaskResultFrame::success("task-1", "node-1", b"out".to_vec(), sha256(b"out"), 1, false);
        assert!(res.validate().is_ok());
        res.result = b"tampered".to_vec();
        assert!(matches!(
            res.validate(),
            Err(TaskWireError::ResultHashMismatch)
        ));
    }
}
