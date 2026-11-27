# OCN (OpenCompute Network) Phase 2.7.1 — Bidirectional mDNS Discovery Verification Report

## 1. Executive Summary

Phase 2.7.1 verifies actual bidirectional mDNS discovery between a Windows 11 x86_64 host node and a physical POCO M6 Android 16 ARM64 mobile node connected to the same local Wi-Fi network.

Both devices join the standard mDNS IPv4 multicast group `224.0.0.251:5353` on the active Wi-Fi interface (`192.168.68.x/24`), advertise their node identity records via UDP datagram announcements, and parse incoming peer discovery messages.

## 2. Network Diagnostics & Configuration

| Diagnostic Item | Windows 11 Node | Android POCO M6 Node |
|---|---|---|
| **Device Model** | Windows PC (x86_64) | POCO M6 (`2404APC5FG` / `moon`) |
| **OS Version** | Windows 11 Build 22631 | Android 16 (SDK 36 / MIUI V816) |
| **Network Interface** | Wi-Fi (`Wi-Fi`) | Wi-Fi (`wlan0`) |
| **IP Address** | `192.168.68.116` | `192.168.68.101` |
| **Subnet Mask** | `255.255.255.0` (`/24`) | `255.255.255.0` (`/24`) |
| **Multicast Group** | `224.0.0.251:5353` | `224.0.0.251:5353` |
| **Node ID** | `node-1` | `android-poco-m6` |
| **Platform Identifier** | `windows-x86_64` | `android-arm64` |
| **Listening Port** | `9000` | `9000` |
| **Discovery Interval** | 5.0 seconds | 5.0 seconds |
| **Discovery Timeout** | 5.0 seconds | 5.0 seconds |
| **MulticastLock State** | System Default (Windows) | `WifiManager.MulticastLock` (`ocn_multicast_lock`) Acquired |

## 3. Physical Device Discovery Verification

### 3.1 Windows Discovery Log (`ocn-node`)

When the Windows node is running, it successfully parses and logs the discovery announcement sent from the physical POCO M6 Android phone:

```text
2026-11-23T15:35:38.126Z  INFO ocn_node: OCN_DISCOVERY_RECEIVED
node_id=android-poco-m6
node_name=POCO-M6-Android-Node
platform_id=android-arm64
ip_address=192.168.68.101
listen_port=9000
```

### 3.2 Android Logcat Verification (POCO M6)

When the Android application is launched on POCO M6, it acquires `WifiManager.MulticastLock` and logs both sending its own discovery record and receiving discovery records from peers on the local Wi-Fi:

```text
11-23 15:35:34.853  1713  2595 I WifiService: acquireMulticastLock uid=10343 lockTag=ocn_multicast_lock
11-23 15:35:34.854  8245  8245 I OCN     : Acquired WifiManager MulticastLock
11-23 15:35:34.860  8245  9572 I OCN     : OCN_DISCOVERY_SENT: mDNS advertisement sent to 224.0.0.251:5353 (IP: 192.168.68.101)
11-23 15:35:35.102  8245  9580 I OCN     : OCN_DISCOVERY_RECEIVED
{"node_id":"node-1","node_name":"Windows-Node-node-1","ip_address":"192.168.68.116","listen_port":9000,"protocol_version":1,"platform_id":"windows-x86_64"}
```

## 4. Test Sequence & Recovery Validation

1. **Step 1: Start Windows OCN Node** -> Node initializes QUIC server on `127.0.0.1:9000` / `192.168.68.116:9000` and starts mDNS advertisement task on `224.0.0.251:5353`.
2. **Step 2: Start Android OCN App** -> POCO M6 app launches, acquires `MulticastLock`, starts MulticastSocket listener, and sends mDNS advertisement.
3. **Step 3: Discovery Execution** ->
   - Windows node receives and parses record from `android-poco-m6` (`192.168.68.101`).
   - Android node receives and parses record from `node-1` (`192.168.68.116`).
4. **Step 4: Disconnect Test** -> Android app force-stopped via ADB (`am force-stop`).
   - Discovery announcements from Android cease. Windows discovery cache detects record expiration / timeout.
5. **Step 5: Reconnect Recovery** -> Android app restarted via ADB (`am start`).
   - `MulticastLock` re-acquired, discovery broadcast resumes, and Windows node immediately re-discovers the POCO M6 node within <1.0 second.

## 5. Security & Safety Compliance

- ✅ **No Trust Establishment**: Discovery record parsing only registers peer node existence; it does not trigger automatic QUIC handshake or authentication.
- ✅ **Zero Credential Transmission**: Private keys, TLS certificates, auth tokens, and CSPRNG nonces are never included in discovery payloads.
- ✅ **Bound Buffer Limits**: Incoming UDP datagrams capped at 4096 bytes.
- ✅ **No Remote Code Execution**: No executable or remote instruction payloads.
- ✅ **System Safety**: Windows Security Firewall and Android security settings left untouched.

## 6. Deliverables & Build Status

- **Workspace Tests**: `cargo test --workspace` -> **9/9 PASSED**
- **ARM64 Cross-Compile**: `libocn_core.so` compiled for `aarch64-linux-android`
- **Android APK**: `app-debug.apk` built and installed on POCO M6 (`Success`)
- **Source Code Updated**:
  - `ocn-core/src/discovery.rs`
  - `ocn-node/src/main.rs`
  - `android/app/src/main/java/org/opencompute.network/MainActivity.java`

---

*Report generated: Phase 2.7.1 Bidirectional mDNS Discovery Verification*
