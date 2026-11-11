//! The cryptography of every connection.
//!
//! Connections use TLS 1.3 inside QUIC, with the aws-lc-rs implementation of
//! rustls. The key exchange is the hybrid X25519MLKEM768: classic X25519
//! combined with ML-KEM-768, the post-quantum key encapsulation standardised
//! by NIST as FIPS 203. A recording of today's traffic stays secret unless
//! both are broken, so even a future quantum computer is not enough.
//!
//! The classic groups stay on the list after it. Two Szept devices always
//! settle on the hybrid, and an attacker cannot push them down to X25519: the
//! whole handshake is signed with both identity keys. The classic groups only
//! serve the iroh helper servers (relays, address lookup), which share this
//! configuration, and older Szept versions.

use std::sync::Arc;

use rustls::crypto::{
    CryptoProvider,
    aws_lc_rs::{self, kx_group},
};

/// The rustls configuration every endpoint uses.
pub fn provider() -> Arc<CryptoProvider> {
    let mut provider = aws_lc_rs::default_provider();
    provider.kx_groups = vec![
        kx_group::X25519MLKEM768,
        kx_group::X25519,
        kx_group::SECP256R1,
        kx_group::SECP384R1,
    ];
    Arc::new(provider)
}

#[cfg(test)]
mod tests {
    use std::{net::Ipv4Addr, time::Duration};

    use iroh::{Endpoint, EndpointAddr, RelayMode, endpoint::presets};
    use rustls::{NamedGroup, crypto::SupportedKxGroup};
    use tokio::time::timeout;

    use super::*;

    const ALPN: &[u8] = b"szept-test/pq";

    fn only(group: &'static dyn SupportedKxGroup) -> Arc<CryptoProvider> {
        let mut provider = aws_lc_rs::default_provider();
        provider.kx_groups = vec![group];
        Arc::new(provider)
    }

    async fn endpoint(provider: Arc<CryptoProvider>) -> Endpoint {
        Endpoint::builder(presets::Minimal)
            .relay_mode(RelayMode::Disabled)
            .crypto_provider(provider)
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await
            .unwrap()
    }

    fn loopback(endpoint: &Endpoint) -> EndpointAddr {
        let port = endpoint
            .bound_sockets()
            .into_iter()
            .find(|socket| socket.is_ipv4())
            .unwrap()
            .port();
        EndpointAddr::new(endpoint.id()).with_ip_addr((Ipv4Addr::LOCALHOST, port).into())
    }

    /// Accepts connections forever, keeping each open briefly.
    fn serve(endpoint: Endpoint) {
        tokio::spawn(async move {
            while let Some(incoming) = endpoint.accept().await {
                tokio::spawn(async move {
                    if let Ok(conn) = incoming.await {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        drop(conn);
                    }
                });
            }
        });
    }

    async fn connects(client: &Endpoint, server: &Endpoint) -> bool {
        matches!(
            timeout(
                Duration::from_secs(10),
                client.connect(loopback(server), ALPN)
            )
            .await,
            Ok(Ok(_))
        )
    }

    #[test]
    fn the_hybrid_comes_first() {
        assert_eq!(provider().kx_groups[0].name(), NamedGroup::X25519MLKEM768);
    }

    /// A peer that accepts nothing but the hybrid proves what Szept really
    /// negotiates: if the connection comes up, it is post-quantum.
    #[tokio::test]
    async fn szept_negotiates_the_hybrid_in_both_roles() {
        let strict = endpoint(only(kx_group::X25519MLKEM768)).await;
        let classic = endpoint(only(kx_group::X25519)).await;
        let szept = endpoint(provider()).await;
        serve(strict.clone());
        serve(szept.clone());

        // The check is real: a classic-only client cannot reach the strict peer.
        assert!(!connects(&classic, &strict).await);
        // Szept dialling the strict peer, and the strict peer dialling Szept.
        assert!(connects(&szept, &strict).await);
        assert!(connects(&strict, &szept).await);
        // Older, classic-only versions can still reach Szept.
        assert!(connects(&classic, &szept).await);
    }
}
