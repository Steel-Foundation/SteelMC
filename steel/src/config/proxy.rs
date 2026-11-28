//! `[server.proxy]` settings for running behind Velocity or `BungeeCord`.

use std::{fmt, fs, path::Path};

use serde::Deserialize;
use steel_core::config::{ForwardingSecret, PlayerInfoForwarding, ProxyConfig};

const MISSING_SECRET: &str = "proxy.forwarding_secret or proxy.forwarding_secret_file is required for the bungeeguard and modern forwarding modes";

/// Player info forwarding mode, named like Velocity's `player-info-forwarding-mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayerInfoForwardingMode {
    /// No proxy.
    None,
    /// `BungeeCord` `ip_forward` / Velocity `legacy`.
    Legacy,
    /// Legacy forwarding guarded by a `BungeeGuard` token.
    BungeeGuard,
    /// Velocity modern forwarding.
    Modern,
}

impl PlayerInfoForwardingMode {
    const fn needs_secret(self) -> bool {
        matches!(self, Self::BungeeGuard | Self::Modern)
    }
}

/// `[server.proxy]` as written in `config.toml`.
#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProxySettings {
    /// Must match the proxy's forwarding mode.
    pub player_info_forwarding_mode: PlayerInfoForwardingMode,
    /// Inline forwarding secret or `BungeeGuard` token.
    pub forwarding_secret: Option<String>,
    /// File holding the forwarding secret, like Velocity's `forwarding.secret`.
    pub forwarding_secret_file: Option<String>,
    /// Whether the proxy runs in online mode.
    pub online_mode: bool,
}

impl Default for ProxySettings {
    fn default() -> Self {
        Self {
            player_info_forwarding_mode: PlayerInfoForwardingMode::None,
            forwarding_secret: None,
            forwarding_secret_file: None,
            online_mode: true,
        }
    }
}

impl fmt::Debug for ProxySettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProxySettings")
            .field(
                "player_info_forwarding_mode",
                &self.player_info_forwarding_mode,
            )
            .field(
                "forwarding_secret",
                &self.forwarding_secret.as_ref().map(|_| "<redacted>"),
            )
            .field("forwarding_secret_file", &self.forwarding_secret_file)
            .field("online_mode", &self.online_mode)
            .finish()
    }
}

impl ProxySettings {
    /// Whether any forwarding mode is enabled.
    #[must_use]
    pub fn is_forwarding(&self) -> bool {
        self.player_info_forwarding_mode != PlayerInfoForwardingMode::None
    }

    /// Checks the settings without touching the filesystem.
    pub(super) fn validate(&self) -> Result<(), &'static str> {
        let mode = self.player_info_forwarding_mode;
        if self.forwarding_secret.is_some() && self.forwarding_secret_file.is_some() {
            return Err("set only one of proxy.forwarding_secret and proxy.forwarding_secret_file");
        }
        if !mode.needs_secret() {
            return Ok(());
        }
        if self.forwarding_secret.is_none() && self.forwarding_secret_file.is_none() {
            return Err(MISSING_SECRET);
        }
        if self
            .forwarding_secret
            .as_deref()
            .is_some_and(|secret| secret.trim().is_empty())
        {
            return Err("proxy.forwarding_secret must not be empty");
        }
        Ok(())
    }

    /// Converts the settings to the runtime form, reading `forwarding_secret_file` if set.
    /// Relative paths resolve against the working directory.
    ///
    /// # Errors
    /// Fails closed when a secret mode has no usable secret, instead of falling back to
    /// unauthenticated forwarding.
    pub(super) fn into_runtime(self) -> Result<ProxyConfig, String> {
        self.validate()?;
        let online_mode = self.online_mode;
        let forwarding = match self.player_info_forwarding_mode {
            PlayerInfoForwardingMode::None => PlayerInfoForwarding::None,
            PlayerInfoForwardingMode::Legacy => PlayerInfoForwarding::Legacy,
            PlayerInfoForwardingMode::BungeeGuard => {
                PlayerInfoForwarding::BungeeGuard(self.into_secret()?)
            }
            PlayerInfoForwardingMode::Modern => PlayerInfoForwarding::Modern(self.into_secret()?),
        };
        Ok(ProxyConfig {
            forwarding,
            online_mode,
        })
    }

    /// Resolves the inline secret or reads it from `forwarding_secret_file`.
    fn into_secret(self) -> Result<ForwardingSecret, String> {
        let (secret, source) = match (self.forwarding_secret, self.forwarding_secret_file) {
            (Some(secret), _) => (secret, "proxy.forwarding_secret".to_owned()),
            (None, Some(path)) => {
                let secret = fs::read_to_string(Path::new(&path)).map_err(|error| {
                    format!(
                        "failed to read proxy.forwarding_secret_file {path}: {error} (copy forwarding.secret from your Velocity directory, or your BungeeGuard token, into this file)"
                    )
                })?;
                // Velocity joins the file's lines: line breaks are dropped, other whitespace is kept.
                (
                    secret.replace(['\r', '\n'], ""),
                    format!("proxy.forwarding_secret_file {path}"),
                )
            }
            (None, None) => return Err(MISSING_SECRET.to_owned()),
        };
        let blank = secret.trim().is_empty();
        ForwardingSecret::new(secret.into_bytes())
            .filter(|_| !blank)
            .ok_or_else(|| format!("{source} must not be empty"))
    }
}
