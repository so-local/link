//! TLS to a route's one host, trusting what this machine trusts: what a
//! route's bridge speaks to its host itself (a hosted model's provider, a
//! secret's service), after putting in what the program never holds.

use std::sync::{Arc, OnceLock};

/// A test's own authority, trusted too (a debug build only): the
/// variable holds its PEM file.
pub const TEST_CA: &str = "LNK_TEST_CA";

/// [`TEST_CA`]'s file, in a debug build that's given one.
pub fn test_ca() -> Option<String> {
    link_secretive::test_var(TEST_CA).and_then(|v| v.into_string().ok())
}

/// A TLS client trusting what this machine trusts.
pub fn connector() -> tokio_rustls::TlsConnector {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    let config = CONFIG.get_or_init(|| {
        link_fetch::install_crypto_provider();
        use rustls_platform_verifier::ConfigVerifierExt;
        // A test's own authority too, in a debug build that's given one.
        if let Some(ca) = test_ca() {
            use rustls::pki_types::pem::PemObject;
            let roots: Vec<_> = rustls::pki_types::CertificateDer::pem_file_iter(&ca)
                .expect("the test's authority")
                .flatten()
                .collect();
            let provider = rustls::crypto::CryptoProvider::get_default()
                .expect("a crypto provider")
                .clone();
            let verifier =
                rustls_platform_verifier::Verifier::new_with_extra_roots(roots, provider.clone())
                    .expect("this machine's certificate store");
            return Arc::new(
                rustls::ClientConfig::builder_with_provider(provider)
                    .with_safe_default_protocol_versions()
                    .expect("TLS versions")
                    .dangerous()
                    .with_custom_certificate_verifier(Arc::new(verifier))
                    .with_no_client_auth(),
            );
        }
        Arc::new(
            rustls::ClientConfig::with_platform_verifier()
                .expect("this machine's certificate store"),
        )
    });
    tokio_rustls::TlsConnector::from(config.clone())
}

/// A TLS connection to `host` through `upstream`: dialed as Link's own
/// call to its one host ([`super::dial::dial_out`]).
pub async fn connect(
    host: &str,
    upstream: &'static crate::net::Upstream,
) -> anyhow::Result<super::pool::Kept> {
    let tcp = super::dial::dial_out(host, 443, upstream).await?;
    let name = rustls::pki_types::ServerName::try_from(host.to_string())?;
    Ok(connector().connect(name, tcp).await?)
}
