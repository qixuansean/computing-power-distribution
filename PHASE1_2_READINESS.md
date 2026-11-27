# OCN Phase 1.2 — Cross-Platform Readiness Audit

## 1. Executive Summary
This audit evaluates the OCN (OpenCompute Network) codebase for cross-platform readiness, specifically preparing the architecture for Windows and Android node communication without initiating full Android UI or GPU compute implementations.

---

## 2. Protocol Independence Audit (`ocn-core/src/proto.rs`)

### 2.1 Protocol Specification & Versioning
- **Current Status**: The message payload structure `OCNPacket` wraps `OCNMsgType` enums using `bincode` binary serialization.
- **Protocol Versioning**: Currently lacks an explicit header version field (e.g., `protocol_version: u32`).
- **Encoding Standard**: `bincode` (v1.3) uses fixed endianness (Little Endian), which ensures data alignment consistency across x86_64 (Windows) and ARM64 (Android) architectures.

### 2.2 Fields & Error Rules
- **Header Envelope**: A standard header envelope format `OCNHeader` will be required before Phase 2:
  - `magic_bytes`: `[u8; 4]` (`0x4F, 0x43, 0x4E, 0x31` -> `"OCN1"`)
  - `version`: `u16` (`1`)
  - `msg_type`: `u16`
  - `payload_len`: `u32`
- **Error Propagation**: Current protocol uses string error messages (`message: String`). A structured error code enum (`ErrorCode`) will prevent language/platform parsing ambiguity.

---

## 3. Security Audit & 复核 Findings

### 3.1 Nonce Generation & Replay Protection
- **Audit Target**: `ocn-node/src/main.rs` & `ocn-core/src/proto.rs`
- **Current State**: Nonce is derived from `SystemTime::now().duration_since(UNIX_EPOCH)`.
- **Finding**: System time nonces are vulnerable to clock skew and time-rollback attacks.
- **Remediation Plan**: Replace timestamp nonces with 128-bit cryptographically secure random values generated via `ring::rand::SystemRandom` or `getrandom`.

### 3.2 Private Key Security & Permissions
- **Audit Target**: `ocn-core/src/identity.rs`
- **Windows**: Keys saved to disk (`.key.der`) require Windows ACL restriction (restricted to the executing user's SID).
- **Android**: Keys must be stored in Android Keystore / Internal App Data Directory (`Context.getFilesDir()`), preventing other apps or unrooted access.

### 3.3 TLS 1.3 & Mutual Authentication
- **Verification**: `rustls` 0.23 with `ring` crypto provider correctly mandates ALPN `ocn-quic-1` and validates client/server certificate DER SHA-256 fingerprints.

### 3.4 Pre-Allocation Memory Bounds
- **Verification**: `MAX_PACKET_SIZE` (10 MB) is validated prior to allocating `vec![0u8; len]` buffers in `quic.rs`, effectively neutralizing OOM DoS vector.

---

## 4. Android Compatibility & Capability Degradation Plan

### 4.1 Architecture & Single Binary Approach
- **Primary Target**: `aarch64-linux-android` (ARM64-v8a).
- **Universal Codebase**: Windows CLI (`ocn-node`) and Android Daemon (`libocn_core.so` via JNI / C-FFI) share 100% of network, identity, and protocol logic in `ocn-core`.

### 4.2 Auto-Detection & Feature Degradation Matrix

| Feature | Windows Target | Android Target | Degradation Action on Android |
|:---|:---|:---|:---|
| **Transport** | QUIC over UDP / TCP | QUIC over UDP / Wi-Fi | Fall back to TCP Relay if UDP blocked by mobile carrier. |
| **CPU Capability** | `sysinfo` Native | `/proc/cpuinfo` / `sysinfo` | Restrict background thread pool count to match thermal throttles. |
| **GPU Acceleration** | CUDA / DirectCompute | Vulkan / OpenCL / None | Gracefully disable GPU pipeline; fallback to CPU compute tasks. |
| **Background Execution**| System Service | Foreground Service | Request `FOREGROUND_SERVICE_SPECIAL_USE` permission to keep QUIC alive. |

---

## 5. Build System & Cross-Compilation Plan

### 5.1 Cargo Cross-Compilation Architecture
- **Rust Core**: Compiles to `aarch64-linux-android` shared library (`libocn_core.so`) using `cargo build --target aarch64-linux-android --lib`.
- **NDK Toolchain**: Uses Standalone NDK LLVM Clang linker (`aarch64-linux-android24-clang`) configured in `.cargo/config.toml`.
- **Zero-Android-Studio Requirement**: Pure CLI build using `cargo-ndk` or direct NDK toolchain scripts.

---

## 6. PHASE 1.2 Summary Checklist & Next Steps

1. **Cross-Platform Reusability**: Confirmed. `ocn-core` is 100% OS-agnostic pure Rust.
2. **Modules to Modify in Phase 2**:
   - `proto.rs`: Add explicit `OCNHeader` (Magic, Version, Error Codes).
   - `identity.rs`: Upgrade Nonce generation to CSPRNG (`ring::rand`).
3. **Next Implementation Sequence**:
   - Step 1: Refactor `proto.rs` protocol header.
   - Step 2: Implement CSPRNG Nonce generation.
   - Step 3: Integrate `sysinfo` capability detection in `metrics.rs`.
   - Step 4: Validate ARM64 compilation via Cargo NDK.
