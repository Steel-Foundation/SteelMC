use std::{sync::Arc, time::Duration};

use arc_swap::ArcSwapOption;
use base64::{Engine as _, prelude::BASE64_STANDARD};
use serde::Deserialize;
use steel_crypto::{CryptError, public_key_from_bytes, signature::ProfileKeyValidator};
use thiserror::Error;
use tokio::{sync::oneshot, time::sleep};
use tokio_util::sync::CancellationToken;

const DEFAULT_SERVICES_SERVER: &str = "https://api.minecraftservices.com/publickeys";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const DAILY_REFRESH_INTERVAL: Duration = Duration::from_hours(24);
const BASE_FAILURE_INTERVAL: Duration = Duration::from_mins(5);
const MAX_BACKOFF_EXPONENT: u32 = 6;

#[derive(Debug, Error)]
pub(super) enum ServiceKeyError {
    #[error("invalid services key endpoint '{endpoint}': {reason}")]
    InvalidEndpoint { endpoint: String, reason: String },
    #[error("failed to build services key HTTP client: {0}")]
    Client(#[source] reqwest::Error),
    #[error("services key request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("services key response contains invalid base64: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("services key response contains an invalid public key: {0}")]
    PublicKey(#[from] CryptError),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServiceKeyResponse {
    profile_property_keys: Option<Vec<ServiceKeyData>>,
    player_certificate_keys: Option<Vec<ServiceKeyData>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServiceKeyData {
    public_key: String,
}

/// Cached Mojang service keys used to validate player-key certificates.
pub(super) struct ServiceKeyStore {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    profile_key_validator: ArcSwapOption<ProfileKeyValidator>,
}

impl ServiceKeyStore {
    pub(super) fn new(endpoint: Option<&str>) -> Result<Self, ServiceKeyError> {
        let endpoint = endpoint.unwrap_or(DEFAULT_SERVICES_SERVER);
        let parsed_endpoint =
            reqwest::Url::parse(endpoint).map_err(|error| ServiceKeyError::InvalidEndpoint {
                endpoint: endpoint.to_owned(),
                reason: error.to_string(),
            })?;
        if !matches!(parsed_endpoint.scheme(), "http" | "https") {
            return Err(ServiceKeyError::InvalidEndpoint {
                endpoint: endpoint.to_owned(),
                reason: "expected http or https".to_owned(),
            });
        }
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .build()
            .map_err(ServiceKeyError::Client)?;

        Ok(Self {
            client,
            endpoint: parsed_endpoint,
            profile_key_validator: ArcSwapOption::empty(),
        })
    }

    pub(super) fn profile_key_validator(&self) -> Option<Arc<ProfileKeyValidator>> {
        self.profile_key_validator.load_full()
    }

    /// Starts the initial fetch and returns a signal completed after its first attempt.
    pub(super) fn start(
        self: &Arc<Self>,
        cancel_token: CancellationToken,
    ) -> oneshot::Receiver<()> {
        let (ready_tx, ready_rx) = oneshot::channel();
        let store = Arc::clone(self);
        drop(tokio::spawn(async move {
            let mut has_successful_snapshot = match store.refresh().await {
                Ok(()) => true,
                Err(error) => {
                    log::warn!("Failed to load Minecraft services public keys: {error}");
                    false
                }
            };
            let _ = ready_tx.send(());

            let mut failure_count = 0;
            loop {
                let delay = if has_successful_snapshot {
                    DAILY_REFRESH_INTERVAL
                } else {
                    failure_delay(failure_count)
                };
                tokio::select! {
                    () = cancel_token.cancelled() => return,
                    () = sleep(delay) => {}
                }

                match store.refresh().await {
                    Ok(()) => {
                        has_successful_snapshot = true;
                        failure_count = 0;
                    }
                    Err(error) => {
                        log::warn!("Failed to refresh Minecraft services public keys: {error}");
                        if !has_successful_snapshot {
                            failure_count = failure_count.saturating_add(1);
                        }
                    }
                }
            }
        }));
        ready_rx
    }

    async fn refresh(&self) -> Result<(), ServiceKeyError> {
        let response = self
            .client
            .get(self.endpoint.clone())
            .send()
            .await?
            .error_for_status()?
            .json::<ServiceKeyResponse>()
            .await?;
        let validator = profile_key_validator(response)?;
        self.profile_key_validator.store(validator.map(Arc::new));
        Ok(())
    }
}

fn profile_key_validator(
    response: ServiceKeyResponse,
) -> Result<Option<ProfileKeyValidator>, ServiceKeyError> {
    // Authlib rejects the complete snapshot when either service-key list is malformed.
    parse_keys(response.profile_property_keys)?;
    let keys = parse_keys(response.player_certificate_keys)?;
    Ok(ProfileKeyValidator::new(keys))
}

fn parse_keys(
    keys: Option<Vec<ServiceKeyData>>,
) -> Result<Vec<rsa::RsaPublicKey>, ServiceKeyError> {
    keys.unwrap_or_default()
        .into_iter()
        .map(|key| {
            let der = BASE64_STANDARD.decode(key.public_key)?;
            public_key_from_bytes(&der).map_err(ServiceKeyError::from)
        })
        .collect()
}

fn failure_delay(failure_count: u32) -> Duration {
    let exponent = failure_count.min(MAX_BACKOFF_EXPONENT);
    BASE_FAILURE_INTERVAL.saturating_mul(1 << exponent)
}

#[cfg(test)]
mod tests {
    use base64::{Engine as _, prelude::BASE64_STANDARD};
    use steel_crypto::{generate_key_pair, public_key_to_bytes};

    use super::{
        BASE_FAILURE_INTERVAL, MAX_BACKOFF_EXPONENT, ServiceKeyData, ServiceKeyResponse,
        failure_delay, profile_key_validator,
    };

    #[test]
    fn parses_player_certificate_keys() {
        let (_, public_key) = generate_key_pair().expect("test RSA key should generate");
        let der = public_key_to_bytes(&public_key).expect("test RSA key should encode");
        let response = serde_json::from_value::<ServiceKeyResponse>(serde_json::json!({
            "profilePropertyKeys": [],
            "playerCertificateKeys": [{ "publicKey": BASE64_STANDARD.encode(der) }],
        }))
        .expect("Minecraft services response should deserialize");

        assert!(
            profile_key_validator(response)
                .expect("valid response should parse")
                .is_some()
        );
    }

    #[test]
    fn empty_player_certificate_keys_disable_validation() {
        let response = ServiceKeyResponse {
            profile_property_keys: None,
            player_certificate_keys: None,
        };

        assert!(
            profile_key_validator(response)
                .expect("missing keys should be accepted")
                .is_none()
        );
    }

    #[test]
    fn malformed_player_certificate_key_rejects_snapshot() {
        let response = ServiceKeyResponse {
            profile_property_keys: None,
            player_certificate_keys: Some(vec![ServiceKeyData {
                public_key: "not base64".to_owned(),
            }]),
        };

        assert!(profile_key_validator(response).is_err());
    }

    #[test]
    fn malformed_profile_property_key_rejects_snapshot() {
        let (_, public_key) = generate_key_pair().expect("test RSA key should generate");
        let der = public_key_to_bytes(&public_key).expect("test RSA key should encode");
        let response = ServiceKeyResponse {
            profile_property_keys: Some(vec![ServiceKeyData {
                public_key: "not base64".to_owned(),
            }]),
            player_certificate_keys: Some(vec![ServiceKeyData {
                public_key: BASE64_STANDARD.encode(der),
            }]),
        };

        assert!(profile_key_validator(response).is_err());
    }

    #[test]
    fn initial_failures_use_authlib_backoff_cap() {
        assert_eq!(failure_delay(0), BASE_FAILURE_INTERVAL);
        assert_eq!(failure_delay(1), BASE_FAILURE_INTERVAL * 2);
        assert_eq!(
            failure_delay(MAX_BACKOFF_EXPONENT + 1),
            BASE_FAILURE_INTERVAL * (1 << MAX_BACKOFF_EXPONENT)
        );
    }
}
