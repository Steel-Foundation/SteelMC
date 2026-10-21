use std::{fs, path::Path};

use steel_core::whitelist::WhitelistConfig;

const DEFAULT_WHITELIST: &str = "enabled = false\nentries = []\n";

pub(super) fn load_or_create_whitelist(path: &Path) -> Result<WhitelistConfig, String> {
    if path.exists() {
        let contents = fs::read_to_string(path)
            .map_err(|error| format!("failed to read whitelist {}: {error}", path.display()))?;
        toml::from_str(&contents)
            .map_err(|error| format!("failed to parse whitelist {}: {error}", path.display()))
    } else {
        fs::write(path, DEFAULT_WHITELIST)
            .map_err(|error| format!("failed to write whitelist {}: {error}", path.display()))?;
        Ok(WhitelistConfig::default())
    }
}
