//! `[server.proxy]` settings for running behind Velocity or `BungeeCord`.

use std::{fmt, fs, path::Path};

use serde::Deserialize;
use steel_core::config::{ForwardingSecret, PlayerInfoForwarding, ProxyConfig};

const MISSING_SECRET: &str = "proxy forwarding secret must not be empty";

/// Player info forwarding mode, named like Velocity's `player-info-forwarding-mode`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayerInfoForwardingMode {
    /// No proxy.
    #[default]
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
            return Err(
                "proxy.forwarding_secret or proxy.forwarding_secret_file is required for the bungeeguard and modern forwarding modes",
            );
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

    /// Reads `forwarding_secret_file` into `forwarding_secret` so the runtime conversion
    /// never touches the filesystem. Relative paths resolve against the working directory.
    pub(super) fn load_secret_file(&mut self) -> Result<(), String> {
        if !self.player_info_forwarding_mode.needs_secret() {
            return Ok(());
        }
        let Some(path) = self.forwarding_secret_file.take() else {
            return Ok(());
        };
        let secret = fs::read_to_string(Path::new(&path)).map_err(|error| {
            format!(
                "failed to read proxy.forwarding_secret_file {path}: {error} (copy forwarding.secret from your Velocity directory, or your BungeeGuard token, into this file)"
            )
        })?;
        if secret.trim().is_empty() {
            return Err(format!("proxy.forwarding_secret_file {path} is empty"));
        }
        self.forwarding_secret = Some(secret);
        Ok(())
    }

    /// Converts loaded settings to the runtime form.
    ///
    /// Surrounding whitespace is trimmed from the secret, matching how Velocity reads
    /// `forwarding.secret`.
    ///
    /// # Errors
    /// Fails closed when a secret mode has no loaded secret, instead of falling back to
    /// unauthenticated forwarding.
    pub(super) fn into_runtime(self) -> Result<ProxyConfig, &'static str> {
        self.validate()?;
        if self.player_info_forwarding_mode.needs_secret() && self.forwarding_secret_file.is_some()
        {
            return Err("proxy.forwarding_secret_file was not loaded");
        }
        let secret = self
            .forwarding_secret
            .and_then(|secret| ForwardingSecret::new(secret.trim().as_bytes()));
        let forwarding = match self.player_info_forwarding_mode {
            PlayerInfoForwardingMode::None => PlayerInfoForwarding::None,
            PlayerInfoForwardingMode::Legacy => PlayerInfoForwarding::Legacy,
            PlayerInfoForwardingMode::BungeeGuard => {
                PlayerInfoForwarding::BungeeGuard(secret.ok_or(MISSING_SECRET)?)
            }
            PlayerInfoForwardingMode::Modern => {
                PlayerInfoForwarding::Modern(secret.ok_or(MISSING_SECRET)?)
            }
        };
        Ok(ProxyConfig {
            forwarding,
            online_mode: self.online_mode,
        })
    }
}
