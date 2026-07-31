use std::{
    env,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use steel_core::{
    config::WorldsConfig,
    permission::{
        PermissionGroupConfig, PermissionGroupStore, PermissionGroups, PermissionGroupsConfig,
        PermissionMetadataRuleConfig, PermissionMetadataValue,
    },
};
use tokio::fs as async_fs;

use super::{
    DEFAULT_CONFIG, DEFAULT_WORLDS, FilePermissionGroupStore, LogLevel, RotationTimeFormat,
    SteelConfig,
    groups::{DEFAULT_GROUPS, GROUPS_CONFIG_HEADER},
    server::validate,
};

fn temp_config_root(name: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    env::temp_dir().join(format!("steelmc-config-{name}-{suffix}"))
}

#[test]
fn packaged_configs_parse() {
    let config: SteelConfig = toml::from_str(DEFAULT_CONFIG).expect("default config parses");
    assert!(DEFAULT_CONFIG.starts_with(concat!(
        "#:schema https://raw.githubusercontent.com/Steel-Foundation/SteelMC/refs/heads/master/",
        "package-content/config.schema.json\n",
        "# Documentation: https://steelmc.dev/configuration/server-configuration/\n\n",
    )));
    assert!(!config.server.allow_flight);
    assert_eq!(config.server.chat_spam_threshold_seconds, 10);
    assert_eq!(config.server.command_spam_threshold_seconds, 10);
    validate(&config.server).expect("default config validates");
    let worlds: WorldsConfig = toml::from_str(DEFAULT_WORLDS).expect("default worlds parses");
    assert!(DEFAULT_WORLDS.starts_with(concat!(
        "#:schema https://raw.githubusercontent.com/Steel-Foundation/SteelMC/refs/heads/master/",
        "package-content/worlds.schema.json\n",
        "# Documentation: https://steelmc.dev/configuration/world-configuration/\n\n",
    )));
    assert!(!worlds.domains.is_empty());
    let groups: PermissionGroupsConfig =
        toml::from_str(DEFAULT_GROUPS).expect("default groups parse");
    PermissionGroups::from_config(groups).expect("default groups validate");
    assert!(DEFAULT_GROUPS.starts_with(GROUPS_CONFIG_HEADER));
}

#[test]
fn packaged_schema_declares_server_thread_settings() {
    let Ok(schema) = serde_json::from_str::<serde_json::Value>(include_str!(
        "../../../package-content/config.schema.json"
    )) else {
        panic!("packaged config schema should be valid JSON");
    };
    let Some(thread_properties) = schema
        .pointer("/properties/server/properties/threads/properties")
        .and_then(serde_json::Value::as_object)
    else {
        panic!("packaged config schema should define server thread properties");
    };

    assert!(thread_properties.contains_key("packet_workers"));
    assert!(thread_properties.contains_key("chunk_encoding"));
}

#[tokio::test]
async fn file_permission_group_store_round_trips_typed_config() {
    let root = temp_config_root("groups-store");
    let path = root.join("groups.toml");
    let store = FilePermissionGroupStore::new(path.clone());
    let mut config = PermissionGroupsConfig::default();
    config.groups.insert(
        "builder".to_owned(),
        PermissionGroupConfig {
            allow: vec!["steel.build{domain=survival}".to_owned()],
            metadata: vec![
                PermissionMetadataRuleConfig {
                    key: "plugin:max_homes{domain=survival}".to_owned(),
                    value: PermissionMetadataValue::Integer(5),
                },
                PermissionMetadataRuleConfig {
                    key: "plugin:prefix".to_owned(),
                    value: PermissionMetadataValue::String("[Builder]".to_owned()),
                },
                PermissionMetadataRuleConfig {
                    key: "plugin:can_claim".to_owned(),
                    value: PermissionMetadataValue::Bool(true),
                },
            ],
            ..PermissionGroupConfig::default()
        },
    );

    PermissionGroupStore::save_groups(&store, config.clone())
        .await
        .expect("groups config should save");
    let written = async_fs::read_to_string(&path)
        .await
        .expect("groups config should be written");
    let parsed: PermissionGroupsConfig =
        toml::from_str(&written).expect("written config should parse");

    assert_eq!(
        written.lines().next(),
        Some(concat!(
            "#:schema https://raw.githubusercontent.com/Steel-Foundation/SteelMC/refs/heads/master/",
            "package-content/groups.schema.json"
        ))
    );
    assert!(written.contains("# Documentation: https://steelmc.dev/configuration/permissions/"));
    assert!(written.contains("{ key = \"plugin:max_homes{domain=survival}\", value = 5 }"));
    assert!(written.contains("{ key = \"plugin:prefix\", value = \"[Builder]\" }"));
    assert!(written.contains("{ key = \"plugin:can_claim\", value = true }"));
    assert_eq!(parsed, config);
    PermissionGroups::from_config(parsed).expect("written groups should validate");
    async_fs::remove_dir_all(root)
        .await
        .expect("temporary config directory should be removable");
}

#[test]
fn server_config_defaults_backward_compatible_fields() {
    let input = r#"
            [server]
            server_port = 25565
            max_players = 20
            view_distance = 10
            simulation_distance = 10
            online_mode = true
            encryption = true
            motd = "A Steel Server"
            use_favicon = false
            favicon = "config/favicon.png"
            enforce_secure_chat = false
            chat_spam_threshold_seconds = 10
            command_spam_threshold_seconds = 10
        "#;

    let config: SteelConfig = toml::from_str(input).expect("config should parse");

    assert!(!config.server.allow_flight);
    assert_eq!(config.server.max_chained_neighbor_updates, 1_000_000);
}

#[test]
fn configured_auth_server_flows_to_runtime_config() {
    let auth_server = "https://auth.example.com/session/minecraft/hasJoined";
    let config_toml = DEFAULT_CONFIG.replace(
        "online_mode = true",
        &format!("online_mode = true\nauth_server = \"{auth_server}\""),
    );
    let config: SteelConfig = toml::from_str(&config_toml).expect("config parses");

    assert_eq!(config.server.auth_server.as_deref(), Some(auth_server));
    assert_eq!(
        config.server.into_runtime_config().auth_server.as_deref(),
        Some(auth_server)
    );
}

#[test]
fn configured_profile_server_flows_to_runtime_config() {
    let profile_server = "https://profiles.example.com/lookup/name";
    let config_toml = DEFAULT_CONFIG.replace(
        "online_mode = true",
        &format!("online_mode = true\nprofile_server = \"{profile_server}\""),
    );
    let config: SteelConfig = toml::from_str(&config_toml).expect("config parses");

    assert_eq!(
        config.server.profile_server.as_deref(),
        Some(profile_server)
    );
    assert_eq!(
        config
            .server
            .into_runtime_config()
            .profile_server
            .as_deref(),
        Some(profile_server)
    );
}

#[test]
fn configured_thread_counts_parse_and_flow_to_runtime_config() {
    let config_toml = DEFAULT_CONFIG
        .replace("main_runtime = 0", "main_runtime = 3")
        .replace("chunk_runtime = 0", "chunk_runtime = 4")
        .replace("packet_workers = 0", "packet_workers = 5")
        .replace("chunk_generation = 0", "chunk_generation = 6")
        .replace("chunk_encoding = 0", "chunk_encoding = 7");
    let config: SteelConfig = toml::from_str(&config_toml).expect("config parses");

    assert_eq!(config.server.threads.main_runtime, Some(3));
    assert_eq!(config.server.threads.chunk_runtime, Some(4));
    assert_eq!(config.server.threads.packet_workers, Some(5));
    assert_eq!(config.server.threads.chunk_generation, Some(6));
    assert_eq!(config.server.threads.chunk_encoding, Some(7));
    assert_eq!(
        config.server.clone().into_runtime_config().packet_workers,
        Some(5)
    );
    let runtime_config = config.server.into_runtime_config();
    assert_eq!(runtime_config.chunk_generation_threads, Some(6));
    assert_eq!(runtime_config.chunk_encoding_threads, Some(7));
}

#[test]
fn validate_rejects_extended_view_distance_without_opt_in() {
    let config_toml = DEFAULT_CONFIG.replace("view_distance = 10", "view_distance = 33");
    let config: SteelConfig = toml::from_str(&config_toml).expect("config parses");

    assert_eq!(
        validate(&config.server),
        Err("View distance must in range 1..32")
    );
}

#[test]
fn validate_rejects_online_mode_without_encryption() {
    let config_toml = DEFAULT_CONFIG.replace("encryption = true", "encryption = false");
    let config: SteelConfig = toml::from_str(&config_toml).expect("config parses");

    assert_eq!(
        validate(&config.server),
        Err("encryption must be true when online_mode is enabled")
    );
}

#[test]
fn validate_allows_offline_mode_without_encryption() {
    let config_toml = DEFAULT_CONFIG
        .replace("online_mode = true", "online_mode = false")
        .replace("encryption = true", "encryption = false");
    let config: SteelConfig = toml::from_str(&config_toml).expect("config parses");

    validate(&config.server).expect("offline mode does not require encryption");
}

#[test]
fn validate_allows_extended_view_distance_with_opt_in() {
    let config_toml = DEFAULT_CONFIG
        .replace(
            "allow_extended_view_distance = false",
            "allow_extended_view_distance = true",
        )
        .replace("view_distance = 10", "view_distance = 128")
        .replace("simulation_distance = 10", "simulation_distance = 128");
    let config: SteelConfig = toml::from_str(&config_toml).expect("config parses");

    validate(&config.server).expect("extended view distance validates");
}

#[test]
fn validate_rejects_view_distance_above_supported_ticket_range() {
    let config_toml = DEFAULT_CONFIG
        .replace(
            "allow_extended_view_distance = false",
            "allow_extended_view_distance = true",
        )
        .replace("view_distance = 10", "view_distance = 129");
    let config: SteelConfig = toml::from_str(&config_toml).expect("config parses");

    assert_eq!(
        validate(&config.server),
        Err("View distance must in range 1..128")
    );
}

#[test]
fn validate_rejects_invalid_auth_server_url() {
    let config_toml = DEFAULT_CONFIG.replace(
        "online_mode = true",
        "online_mode = true\nauth_server = \"not a url\"",
    );
    let config: SteelConfig = toml::from_str(&config_toml).expect("config parses");

    assert_eq!(
        validate(&config.server),
        Err("auth_server must be an absolute URL")
    );
}

#[test]
fn validate_allows_http_auth_server_url() {
    let config_toml = DEFAULT_CONFIG.replace(
        "online_mode = true",
        "online_mode = true\nauth_server = \"http://localhost:8080/session/minecraft/hasJoined\"",
    );
    let config: SteelConfig = toml::from_str(&config_toml).expect("config parses");

    validate(&config.server).expect("http auth server URL validates");
}

#[test]
fn validate_rejects_unsupported_auth_server_scheme() {
    let config_toml = DEFAULT_CONFIG.replace(
        "online_mode = true",
        "online_mode = true\nauth_server = \"ftp://auth.example.com/session/minecraft/hasJoined\"",
    );
    let config: SteelConfig = toml::from_str(&config_toml).expect("config parses");

    assert_eq!(
        validate(&config.server),
        Err("auth_server must use http or https")
    );
}

#[test]
fn server_config_defaults_spam_thresholds_for_older_configs() {
    let input = r#"
            [server]
            server_port = 25565
            max_players = 20
            view_distance = 10
            simulation_distance = 10
            online_mode = true
            encryption = true
            motd = "A Steel Server"
            use_favicon = false
            favicon = "config/favicon.png"
            enforce_secure_chat = false
        "#;

    let config: SteelConfig = toml::from_str(input).expect("config should parse");

    assert_eq!(config.server.chat_spam_threshold_seconds, 10);
    assert_eq!(config.server.command_spam_threshold_seconds, 10);
}

#[test]
fn log_config_defaults_for_older_configs() {
    let input = r#"
            [server]
            server_port = 25565
            max_players = 20
            view_distance = 10
            simulation_distance = 10
            online_mode = true
            encryption = true
            motd = "A Steel Server"
            use_favicon = false
            favicon = "config/favicon.png"
            enforce_secure_chat = false

            [log]
            time = "uptime"
            module_path = false
            extra = false
        "#;

    let config: SteelConfig = toml::from_str(input).expect("older log config should parse");
    let log_config = config.log.expect("log config should be present");

    assert_eq!(log_config.log_path, "./.logs");
    assert_eq!(log_config.log_level, LogLevel::Info);
    assert!(log_config.log_file);
    assert_eq!(log_config.rotation_time, RotationTimeFormat::Daily);
    assert_eq!(log_config.max_history, 50);
}
