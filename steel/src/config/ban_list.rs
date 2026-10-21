use std::{fs, path::Path};

use steel_core::ban::BanListConfig;

const DEFAULT_BAN_LIST: &str = "bans = []\n";

pub(super) fn load_or_create_ban_list(path: &Path) -> Result<BanListConfig, String> {
    if path.exists() {
        let contents = fs::read_to_string(path)
            .map_err(|error| format!("failed to read ban list {}: {error}", path.display()))?;
        toml::from_str(&contents)
            .map_err(|error| format!("failed to parse ban list {}: {error}", path.display()))
    } else {
        fs::write(path, DEFAULT_BAN_LIST)
            .map_err(|error| format!("failed to write ban list {}: {error}", path.display()))?;
        Ok(BanListConfig::default())
    }
}
