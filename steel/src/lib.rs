//! # Steel
//!
//! The main library for the Steel Minecraft server.

// World and server lifecycle graphs require deeper auto-trait evaluation.
#![recursion_limit = "256"]
use std::sync::atomic::Ordering;
use std::{
    collections::BTreeSet,
    error::Error,
    fmt, fs, io,
    net::{Ipv4Addr, SocketAddrV4},
    path::Path,
    sync::{Arc, OnceLock},
};

use steel_core::{
    GIT_HASH_SHORT,
    bootstrap::init_globals,
    command::CommandRegistry,
    extension::{ExtensionHooks, RegisteredExtensions},
    permission::PermissionGroupManager,
    server::Server,
};
use steel_extension_api::host;
use steel_login::{JavaTcpClient, ServerConnectionSession};
use tokio::{net::TcpListener, runtime::Runtime, select};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

/// Server configuration module.
pub mod config;
/// A module for logging utilities.
pub mod logger;

/// Static access to the server
pub static SERVER: OnceLock<Arc<Server>> = OnceLock::new();

/// The main server struct.
pub struct SteelServer {
    /// The TCP listener for incoming connections.
    pub tcp_listener: TcpListener,
    /// The cancellation token for graceful shutdown.
    pub cancel_token: CancellationToken,
    /// The next client ID to be assigned.
    pub client_id: u64,
    /// The shared server state.
    pub server: Arc<Server>,
    /// Session id UUID state
    pub connection_session: Arc<ServerConnectionSession>,
    /// Initialized native extensions, kept to run their shutdown callbacks.
    pub extensions: host::InitializedExtensions,
}

/// Folder scanned for extension packages and libraries, also holding each extension's data
/// directory (`extensions/<name>/`), relative to the working directory, like Bukkit's
/// `plugins/`.
pub const EXTENSION_DIRECTORY: &str = "extensions";

/// Startup error for expected operational failures.
#[derive(Debug)]
pub enum SteelServerError {
    /// Core server startup failed.
    Core(String),
    /// TCP listener could not bind.
    Bind {
        /// Server port that failed to bind.
        port: u16,
        /// Underlying IO error.
        source: io::Error,
    },
}

impl fmt::Display for SteelServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core(error) => f.write_str(error),
            Self::Bind { port, source } => {
                write!(f, "failed to bind to server port {port}: {source}")
            }
        }
    }
}

impl Error for SteelServerError {}

impl SteelServer {
    /// Creates a new Steel server.
    ///
    pub async fn new(
        chunk_runtime: Arc<Runtime>,
        cancel_token: CancellationToken,
        steel_config: config::SteelConfig,
    ) -> Result<Self, SteelServerError> {
        Self::new_with_commands(
            chunk_runtime,
            cancel_token,
            steel_config,
            CommandRegistry::new(),
        )
        .await
    }

    /// Creates a new Steel server with additional commands registered atomically at startup.
    pub async fn new_with_commands(
        chunk_runtime: Arc<Runtime>,
        cancel_token: CancellationToken,
        steel_config: config::SteelConfig,
        command_registry: CommandRegistry,
    ) -> Result<Self, SteelServerError> {
        log::info!("Starting Steel Server ({GIT_HASH_SHORT})");

        let mut hooks = ExtensionHooks::default();
        let data_root = Path::new(EXTENSION_DIRECTORY);
        let mut sources = Vec::with_capacity(steel_config.extensions.len());
        let mut configured_paths = BTreeSet::new();
        for extension in &steel_config.extensions {
            configured_paths.insert(fs::canonicalize(&extension.path).map_err(|error| {
                SteelServerError::Core(format!(
                    "cannot resolve extension {}: {error}",
                    extension.path.display()
                ))
            })?);
            // An entry without a config table reads extensions/<name>/config.toml.
            let config = if extension.config.is_empty() {
                None
            } else {
                Some(toml::to_string(&extension.config).map_err(|error| {
                    SteelServerError::Core(format!("invalid extension configuration: {error}"))
                })?)
            };
            sources.push(host::ExtensionSource {
                path: extension.path.clone(),
                config,
            });
        }
        // Like Bukkit's plugins/, create the folder on first start so admins see where
        // extensions go.
        if let Err(error) = fs::create_dir_all(data_root) {
            log::warn!("Cannot create the {EXTENSION_DIRECTORY} folder: {error}");
        }
        let discovered = host::discover(data_root).map_err(SteelServerError::Core)?;
        let folder = fs::canonicalize(data_root).unwrap_or_else(|_| data_root.to_path_buf());
        log::info!(
            "Found {} extension packages and libraries in {}",
            discovered.len(),
            folder.display()
        );
        for path in discovered {
            let canonical = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if !configured_paths.contains(&canonical) {
                sources.push(host::ExtensionSource { path, config: None });
            }
        }
        // SAFETY: extensions are trusted native code installed by the administrator; the
        // loader checks both entrypoint ABIs and pins every library for its callbacks' lifetime.
        let report = unsafe { host::load_all(&sources, data_root, &mut hooks) }
            .map_err(SteelServerError::Core)?;
        for reason in &report.skipped {
            log::warn!("Skipped extension {reason}");
        }
        let loaded = report.extensions;
        for extension in &loaded {
            log::info!("Pre-initialized extension {}", extension.name);
        }
        init_globals();
        let mut extensions =
            RegisteredExtensions::new_with_builtins().map_err(SteelServerError::Core)?;
        let commands = command_registry.freeze().map_err(SteelServerError::Core)?;
        let initialized =
            host::initialize_all(loaded, data_root, &mut hooks).map_err(SteelServerError::Core)?;
        for name in initialized.names() {
            log::info!("Initialized extension {name}");
        }
        extensions.hooks = hooks;

        let permission_group_store = steel_config.permission_group_store();
        let server_port = steel_config.server.server_port;
        let worlds_config = steel_config.worlds;
        let permission_groups =
            PermissionGroupManager::new(steel_config.groups, permission_group_store).map_err(
                |error| {
                    SteelServerError::Core(format!("failed to validate groups config: {error}"))
                },
            )?;
        let player_idle_timeout = steel_config.server.player_idle_timeout;
        let runtime_config = steel_config.server.into_runtime_config();

        let server = Server::new_with_extensions(
            chunk_runtime,
            cancel_token.clone(),
            runtime_config,
            worlds_config,
            permission_groups,
            commands,
            extensions,
        )
        .await
        .map_err(SteelServerError::Core)?;

        server
            .player_idle_timeout
            .store(player_idle_timeout, Ordering::Relaxed);

        let tcp_listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, server_port))
            .await
            .map_err(|source| SteelServerError::Bind {
                port: server_port,
                source,
            })?;

        Ok(Self {
            tcp_listener,
            cancel_token,
            client_id: 0,
            server: Arc::new(server),
            connection_session: Arc::new(ServerConnectionSession::default()),
            extensions: initialized,
        })
    }

    /// Starts the server and begins accepting connections.
    pub async fn start(&mut self, task_tracker: TaskTracker) {
        log::info!("Started Steel Server");

        let server = Arc::clone(&self.server);
        let token = self.cancel_token.clone();
        let server_handle = tokio::spawn(async move {
            server.run(token).await;
        });

        loop {
            select! {
                () = self.cancel_token.cancelled() => {
                    break;
                }
                accept_result = self.tcp_listener.accept() => {
                    let Ok((connection, address)) = accept_result else {
                        continue;
                    };
                    if let Err(e) = connection.set_nodelay(true) {
                        log::warn!("Failed to set TCP_NODELAY: {e}");
                    }
                    let (java_client, sender_recv, net_reader) = JavaTcpClient::new(
                        connection,
                        address,
                        self.client_id,
                        self.cancel_token.child_token(),
                        Arc::clone(&self.server),
                        Arc::clone(&self.connection_session),
                        task_tracker.clone(),
                    );
                    self.client_id = self.client_id.wrapping_add(1);
                    log::info!("Accepted connection from Java Edition: {address} (id {})", self.client_id);

                    let java_client = Arc::new(java_client);
                    java_client.start_outgoing_packet_task(sender_recv);
                    java_client.start_incoming_packet_task(net_reader);
                    // Java_client won't drop until the incoming and outcoming task close
                    // So we dont need to care about them here anymore
                }
            }
        }
        let _ = server_handle.await;
    }
}
