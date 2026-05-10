//! Domain-aware loaded world map.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use small_map::FxSmallMap;
use steel_utils::Identifier;

use crate::config::ResolvedDomainConfig;
use crate::world::World;

/// Loaded worlds plus domain defaults.
pub struct WorldMap {
    worlds: FxSmallMap<8, Identifier, Arc<World>>,
    default_domain: String,
    default_worlds: FxHashMap<String, Identifier>,
}

impl WorldMap {
    /// Creates a world map from resolved domain config.
    #[must_use]
    pub fn new(default_domain: String, domains: &[ResolvedDomainConfig]) -> Self {
        let mut default_worlds = FxHashMap::default();
        for domain in domains {
            default_worlds.insert(domain.name.clone(), domain.default_world.clone());
        }
        Self {
            worlds: FxSmallMap::default(),
            default_domain,
            default_worlds,
        }
    }

    /// Inserts a loaded world.
    pub fn insert(&mut self, key: Identifier, world: Arc<World>) {
        self.worlds.insert(key, world);
    }

    /// Returns a world by loaded world identifier.
    #[must_use]
    pub fn get(&self, key: &Identifier) -> Option<&Arc<World>> {
        self.worlds.get(key)
    }

    /// Iterates loaded world values.
    pub fn values(&self) -> impl Iterator<Item = &Arc<World>> {
        self.worlds.values()
    }

    /// Iterates loaded world keys.
    pub fn keys(&self) -> impl Iterator<Item = &Identifier> {
        self.worlds.keys()
    }

    /// Iterates loaded world key/value pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&Identifier, &Arc<World>)> {
        self.worlds.iter()
    }

    /// Returns number of loaded worlds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.worlds.len()
    }

    /// Returns whether there are no loaded worlds.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.worlds.is_empty()
    }

    /// Returns the default domain name.
    #[must_use]
    pub fn default_domain(&self) -> &str {
        &self.default_domain
    }

    /// Returns whether a domain exists.
    #[must_use]
    pub fn has_domain(&self, domain: &str) -> bool {
        self.default_worlds.contains_key(domain)
    }

    /// Iterates domain names.
    pub fn domain_names(&self) -> impl Iterator<Item = &str> {
        self.default_worlds.keys().map(String::as_str)
    }

    /// Returns a domain's default world.
    #[must_use]
    pub fn default_world(&self, domain: &str) -> Option<&Arc<World>> {
        self.default_worlds
            .get(domain)
            .and_then(|key| self.worlds.get(key))
    }

    /// Returns the server default world.
    #[must_use]
    pub fn server_default_world(&self) -> Option<&Arc<World>> {
        self.default_world(self.default_domain())
    }

    /// Returns loaded worlds in the given domain.
    #[must_use]
    pub fn worlds_in_domain(&self, domain: &str) -> Vec<Arc<World>> {
        self.worlds
            .values()
            .filter(|world| world.domain() == domain)
            .cloned()
            .collect()
    }

    /// Resolves a conventional portal target name in the source world's domain.
    #[must_use]
    pub fn resolve_portal_target(
        &self,
        source_world: &World,
        target_world_name: &str,
    ) -> Option<Arc<World>> {
        let key = Identifier::new(
            source_world.domain().to_owned(),
            target_world_name.to_owned(),
        );
        self.worlds.get(&key).cloned()
    }
}
