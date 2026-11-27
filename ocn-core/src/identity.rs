use rcgen::{CertificateParams, KeyPair};
use ring::digest::{Context, SHA256};
use std::sync::{Arc, Mutex, OnceLock};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// A trusted pairing record binding a node_id to its expected certificate fingerprint.
/// Created on first authenticated connection via token, verified on every subsequent connection.
#[derive(Clone, Debug)]
pub struct PeerPairing {
    /// Maps node_id -> expected cert SHA-256 fingerprint (hex string)
    pub node_id_to_fingerprint: HashSet<(String, String)>,
}

impl PeerPairing {
    /// Empty pairing store, to be populated from peers.json or during first auth.
    pub fn new() -> Self {
        Self {
            node_id_to_fingerprint: HashSet::new(),
        }
    }

    /// Check whether the given node_id and fingerprint are trusted according to this pairing store.
    /// Returns Some(true) if matched, Some(false) if mismatched, None if not found (unpaired).
    pub fn check_pairing(&self, node_id: &str, fingerprint: &str) -> Option<bool> {
        if self.node_id_to_fingerprint.contains(&(node_id.to_string(), fingerprint.to_string())) {
            Some(true)
        } else if self
            .node_id_to_fingerprint
            .iter()
            .any(|(n, f)| n == node_id || f == fingerprint)
        {
            // node_id or fingerprint found but not this specific pair -> mismatch
            Some(false)
        } else {
            // completely unknown pair -> unpaired
            None
        }
    }
}

/// Global pairing store for the life of the process.
/// In production this would be loaded from a persistent file (peers.json).
/// For test scenarios, it can be set directly.
pub static PAIRING_STORE: OnceLock<Mutex<PeerPairing>> = OnceLock::new();

pub fn get_pairing_store() -> &'static Mutex<PeerPairing> {
    PAIRING_STORE.get_or_init(|| Mutex::new(PeerPairing::new()))
}

/// Node cryptographic identity: node_id + certificate DER + private key DER + fingerprint.
/// The fingerprint is SHA-256(DER) and is **never** treated as a node_id.
/// The node_id is a separate trust anchor, established via pairing records.
#[derive(Clone)]
pub struct NodeIdentity {
    pub node_id: String,
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
    pub fingerprint: String,
}

impl NodeIdentity {
    /// Generate a new self-signed identity with the given node_id as the DNS SAN.
    /// The fingerprint is computed from the certificate DER and stored separately.
    pub fn generate(node_id: &str) -> anyhow::Result<Self> {
        let mut params = CertificateParams::default();
        params.subject_alt_names = vec![rcgen::SanType::DnsName(node_id.try_into()?)];
        let key_pair = KeyPair::generate()?;
        let cert = params.self_signed(&key_pair)?;
        let cert_der = cert.der().to_vec();
        let key_der = key_pair.serialize_der();

        let mut context = Context::new(&SHA256);
        context.update(&cert_der);
        let digest = context.finish();
        let fingerprint = hex::encode(digest.as_ref());

        Ok(Self {
            node_id: node_id.to_string(),
            cert_der,
            key_der,
            fingerprint,
        })
    }

    /// Load identity from persisted DER files, or generate if not present.
    /// Storage directory is typically %TEMP%\ocn_identities.
    pub fn load_or_generate<P: AsRef<Path>>(node_id: &str, storage_dir: P) -> anyhow::Result<Self> {
        let dir = storage_dir.as_ref();
        fs::create_dir_all(dir)?;
        let cert_path = dir.join(format!("{}.cert.der", node_id));
        let key_path = dir.join(format!("{}.key.der", node_id));

        if cert_path.exists() && key_path.exists() {
            let cert_der = fs::read(&cert_path)?;
            let key_der = fs::read(&key_path)?;

            let mut context = Context::new(&SHA256);
            context.update(&cert_der);
            let digest = context.finish();
            let fingerprint = hex::encode(digest.as_ref());

            Ok(Self {
                node_id: node_id.to_string(),
                cert_der,
                key_der,
                fingerprint,
            })
        } else {
            let identity = Self::generate(node_id)?;
            fs::write(cert_path, &identity.cert_der)?;
            fs::write(key_path, &identity.key_der)?;
            Ok(identity)
        }
    }

    /// Pure function: compute SHA-256 fingerprint from raw DER bytes.
    /// This is a stateless utility; the result is stored in `self.fingerprint` at generation.
    pub fn cert_fingerprint(der: &[u8]) -> String {
        let mut context = Context::new(&SHA256);
        context.update(der);
        hex::encode(context.finish().as_ref())
    }

    /// Returns the fingerprint of this identity's certificate.
    /// Equivalent to `Self::cert_fingerprint(&self.cert_der)`.
    pub fn fingerprint_from_der(&self) -> String {
        Self::cert_fingerprint(&self.cert_der)
    }

    /// Build a quinn ServerConfig that only allows clients whose cert fingerprint
    /// is in the expected_client_fingerprints set (used for B-1b pairing enforcement).
    pub fn to_server_config(&self, expected_client_fingerprints: Option<HashSet<String>>) -> anyhow::Result<quinn::ServerConfig> {
        let cert = rustls::pki_types::CertificateDer::from(self.cert_der.clone());
        let key = rustls::pki_types::PrivateKeyDer::Pkcs8(self.key_der.clone().into());

        #[derive(Debug)]
        struct StrictClientCertVerifier {
            allowed: Option<HashSet<String>>,
        }

        impl rustls::server::danger::ClientCertVerifier for StrictClientCertVerifier {
            fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
                vec![
                    rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
                    rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
                    rustls::SignatureScheme::ED25519,
                    rustls::SignatureScheme::RSA_PSS_SHA256,
                ]
            }

            fn verify_client_cert(
                &self,
                end_entity: &rustls::pki_types::CertificateDer<'_>,
                _intermediates: &[rustls::pki_types::CertificateDer<'_>],
                _now: rustls::pki_types::UnixTime,
            ) -> Result<rustls::server::danger::ClientCertVerified, rustls::Error> {
                let mut ctx = Context::new(&SHA256);
                ctx.update(end_entity.as_ref());
                let digest = ctx.finish();
                let fp = hex::encode(digest.as_ref());

                if let Some(ref set) = self.allowed {
                    if !set.contains(&fp) {
                        return Err(rustls::Error::General(format!("Client fingerprint {} not in paired allowed set", fp)));
                    }
                }
                Ok(rustls::server::danger::ClientCertVerified::assertion())
            }

            fn verify_tls12_signature(
                &self,
                message: &[u8],
                cert: &rustls::pki_types::CertificateDer<'_>,
                dss: &rustls::DigitallySignedStruct,
            ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
                rustls::crypto::verify_tls12_signature(
                    message,
                    cert,
                    dss,
                    &rustls::crypto::ring::default_provider().signature_verification_algorithms,
                )
            }

            fn verify_tls13_signature(
                &self,
                message: &[u8],
                cert: &rustls::pki_types::CertificateDer<'_>,
                dss: &rustls::DigitallySignedStruct,
            ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
                rustls::crypto::verify_tls13_signature(
                    message,
                    cert,
                    dss,
                    &rustls::crypto::ring::default_provider().signature_verification_algorithms,
                )
            }

            fn root_hint_subjects(&self) -> &[rustls::DistinguishedName] {
                &[]
            }
        }

        let verifier = Arc::new(StrictClientCertVerifier { allowed: expected_client_fingerprints });
        let mut server_crypto = rustls::ServerConfig::builder_with_provider(
            Arc::new(rustls::crypto::ring::default_provider())
        )
        .with_safe_default_protocol_versions()?
        .with_client_cert_verifier(verifier)
        .with_single_cert(vec![cert], key)?;

        server_crypto.alpn_protocols = vec![b"ocn-quic-1".to_vec()];
        let server_config = quinn::ServerConfig::with_crypto(Arc::new(quinn::crypto::rustls::QuicServerConfig::try_from(server_crypto)?));
        Ok(server_config)
    }

    /// Build a quinn ClientConfig with an optional expected server fingerprint for pinning.
    pub fn to_client_config(&self, expected_server_fingerprint: Option<String>) -> anyhow::Result<quinn::ClientConfig> {
        let cert = rustls::pki_types::CertificateDer::from(self.cert_der.clone());
        let key = rustls::pki_types::PrivateKeyDer::Pkcs8(self.key_der.clone().into());

        #[derive(Debug)]
        struct StrictServerCertVerifier {
            expected: Option<String>,
        }

        impl rustls::client::danger::ServerCertVerifier for StrictServerCertVerifier {
            fn verify_server_cert(
                &self,
                end_entity: &rustls::pki_types::CertificateDer<'_>,
                _intermediates: &[rustls::pki_types::CertificateDer<'_>],
                _server_name: &rustls::pki_types::ServerName<'_>,
                _os_response: &[u8],
                _now: rustls::pki_types::UnixTime,
            ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
                let mut ctx = Context::new(&SHA256);
                ctx.update(end_entity.as_ref());
                let digest = ctx.finish();
                let fp = hex::encode(digest.as_ref());

                if let Some(ref exp) = self.expected {
                    if fp != *exp {
                        return Err(rustls::Error::General(format!("Server fingerprint mismatch: expected {}, got {}", exp, fp)));
                    }
                }
                Ok(rustls::client::danger::ServerCertVerified::assertion())
            }

            fn verify_tls12_signature(
                &self,
                message: &[u8],
                cert: &rustls::pki_types::CertificateDer<'_>,
                dss: &rustls::DigitallySignedStruct,
            ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
                rustls::crypto::verify_tls12_signature(
                    message,
                    cert,
                    dss,
                    &rustls::crypto::ring::default_provider().signature_verification_algorithms,
                )
            }

            fn verify_tls13_signature(
                &self,
                message: &[u8],
                cert: &rustls::pki_types::CertificateDer<'_>,
                dss: &rustls::DigitallySignedStruct,
            ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
                rustls::crypto::verify_tls13_signature(
                    message,
                    cert,
                    dss,
                    &rustls::crypto::ring::default_provider().signature_verification_algorithms,
                )
            }

            fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
                vec![
                    rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
                    rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
                    rustls::SignatureScheme::ED25519,
                    rustls::SignatureScheme::RSA_PSS_SHA256,
                ]
            }
        }

        let verifier = Arc::new(StrictServerCertVerifier { expected: expected_server_fingerprint });
        let mut client_crypto = rustls::ClientConfig::builder_with_provider(
            Arc::new(rustls::crypto::ring::default_provider())
        )
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_client_auth_cert(vec![cert], key)?;

        client_crypto.alpn_protocols = vec![b"ocn-quic-1".to_vec()];
        let client_config = quinn::ClientConfig::new(Arc::new(quinn::crypto::rustls::QuicClientConfig::try_from(client_crypto)?));
        Ok(client_config)
    }
}