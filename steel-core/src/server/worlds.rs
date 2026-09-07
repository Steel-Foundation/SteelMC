//! Domain-aware loaded world map.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use small_map::FxSmallMap;
use steel_utils::Identifier;

use crate::config::{ResolvedDomainConfig, ResolvedWorldConfig};
use crate::world::{DomainEntityDirectory, World};

pub(crate) const OVERWORLD_WORLD_NAME: &str = "overworld";
pub(crate) const NETHER_WORLD_NAME: &str = "the_nether";
pub(crate) const END_WORLD_NAME: &str = "the_end";

/// Loaded worlds plus domain defaults.
pub struct WorldMap {
    worlds: FxSmallMap<8, Identifier, Arc<World>>,
    default_domain: String,
    default_worlds: FxHashMap<String, Identifier>,
    nether_portal_targets: FxHashMap<Identifier, Identifier>,
    end_portal_targets: FxHashMap<Identifier, Identifier>,
    entity_directories: FxHashMap<String, Arc<DomainEntityDirectory>>,
}

impl WorldMap {
    /// Creates a world map from resolved domain config.
    #[must_use]
    pub fn new(
        default_domain: String,
        domains: &[ResolvedDomainConfig],
        world_configs: &[ResolvedWorldConfig],
    ) -> Self {
        let mut default_worlds = FxHashMap::default();
        for domain in domains {
            default_worlds.insert(domain.name.clone(), domain.default_world.clone());
        }
        let mut nether_portal_targets = FxHashMap::default();
        let mut end_portal_targets = FxHashMap::default();
        for world in world_configs {
            if let Some(target) = &world.nether_portal_target {
                nether_portal_targets.insert(world.key.clone(), target.clone());
            }
            if let Some(target) = &world.end_portal_target {
                end_portal_targets.insert(world.key.clone(), target.clone());
            }
        }
        Self {
            worlds: FxSmallMap::default(),
            default_domain,
            default_worlds,
            nether_portal_targets,
            end_portal_targets,
            entity_directories: FxHashMap::default(),
        }
    }

    /// Inserts a loaded world.
    pub fn insert(&mut self, key: Identifier, world: Arc<World>) {
        if let Some(replaced) = self.worlds.insert(key, Arc::clone(&world))
            && let Some(directory) = self.entity_directories.get(replaced.domain())
        {
            directory.remove_world(&replaced);
        }

        let directory = self
            .entity_directories
            .entry(world.domain().to_owned())
            .or_insert_with(|| Arc::new(DomainEntityDirectory::new()));
        directory.add_world(&world);
        world.set_domain_entity_directory(Arc::clone(directory));
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

    /// Resolves the vanilla Nether portal target in the source world's domain.
    #[must_use]
    pub fn resolve_nether_portal_target(&self, source_world: &World) -> Option<Arc<World>> {
        if let Some(target) = self.nether_portal_targets.get(&source_world.key) {
            return self.worlds.get(target).cloned();
        }

        self.resolve_portal_target(
            source_world,
            nether_portal_target_world_name(source_world.key.path.as_ref()),
        )
    }

    /// Resolves the vanilla End portal target for non-End source worlds.
    ///
    /// End-to-respawn-world transitions depend on the source world's respawn data,
    /// so that branch is intentionally left to the destination calculator.
    #[must_use]
    pub fn resolve_end_entry_portal_target(&self, source_world: &World) -> Option<Arc<World>> {
        if let Some(target) = self.end_portal_targets.get(&source_world.key) {
            return self.worlds.get(target).cloned();
        }

        end_entry_portal_target_world_name(source_world.key.path.as_ref())
            .and_then(|target| self.resolve_portal_target(source_world, target))
    }
}

fn nether_portal_target_world_name(source_world_name: &str) -> &'static str {
    if source_world_name == NETHER_WORLD_NAME {
        OVERWORLD_WORLD_NAME
    } else {
        NETHER_WORLD_NAME
    }
}

fn end_entry_portal_target_world_name(source_world_name: &str) -> Option<&'static str> {
    if source_world_name == END_WORLD_NAME {
        None
    } else {
        Some(END_WORLD_NAME)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::{entity_type::EntityTypeRef, vanilla_entities};
    use steel_utils::ChunkPos;
    use uuid::Uuid;

    use crate::config::ResolvedDomainConfig;
    use crate::entity::{Entity, EntityBase, EntityReference, SharedEntity};
    use crate::test_support::{fresh_test_world_in_domain, insert_ready_full_chunk};
    use crate::world::World;

    use super::{WorldMap, end_entry_portal_target_world_name, nether_portal_target_world_name};

    struct LookupTestEntity {
        base: EntityBase,
    }

    impl LookupTestEntity {
        fn shared(world: &Arc<World>, id: i32, uuid: Uuid) -> SharedEntity {
            Arc::new(Self {
                base: EntityBase::with_uuid(
                    id,
                    uuid,
                    DVec3::ZERO,
                    vanilla_entities::ITEM.dimensions,
                    Arc::downgrade(world),
                ),
            })
        }
    }

    crate::entity::impl_test_downcast_type!(LookupTestEntity);

    impl Entity for LookupTestEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::ITEM
        }
    }

    #[test]
    fn nether_portal_target_names_follow_vanilla_level_keys() {
        assert_eq!(nether_portal_target_world_name("overworld"), "the_nether");
        assert_eq!(nether_portal_target_world_name("the_end"), "the_nether");
        assert_eq!(nether_portal_target_world_name("the_nether"), "overworld");
    }

    #[test]
    fn end_entry_portal_target_name_is_only_for_non_end_sources() {
        assert_eq!(
            end_entry_portal_target_world_name("overworld"),
            Some("the_end")
        );
        assert_eq!(
            end_entry_portal_target_world_name("the_nether"),
            Some("the_end")
        );
        assert_eq!(end_entry_portal_target_world_name("the_end"), None);
    }

    #[test]
    fn entity_references_resolve_within_but_not_across_domains() {
        let alpha_source = fresh_test_world_in_domain("alpha", "overworld");
        let alpha_target = fresh_test_world_in_domain("alpha", "the_nether");
        let beta_target = fresh_test_world_in_domain("beta", "overworld");
        let domains = [
            ResolvedDomainConfig {
                name: "alpha".to_owned(),
                default_world: alpha_source.key.clone(),
                worlds: vec![alpha_source.key.clone(), alpha_target.key.clone()],
            },
            ResolvedDomainConfig {
                name: "beta".to_owned(),
                default_world: beta_target.key.clone(),
                worlds: vec![beta_target.key.clone()],
            },
        ];
        let mut worlds = WorldMap::new("alpha".to_owned(), &domains, &[]);
        for world in [&alpha_source, &alpha_target, &beta_target] {
            worlds.insert(world.key.clone(), Arc::clone(world));
        }

        insert_ready_full_chunk(&alpha_target, ChunkPos::new(0, 0));
        insert_ready_full_chunk(&beta_target, ChunkPos::new(0, 0));
        let alpha_uuid = Uuid::from_u128(1);
        let beta_uuid = Uuid::from_u128(2);
        let alpha_entity = LookupTestEntity::shared(&alpha_target, 1, alpha_uuid);
        let beta_entity = LookupTestEntity::shared(&beta_target, 2, beta_uuid);
        let Ok(()) = alpha_target.try_add_entity(Arc::clone(&alpha_entity)) else {
            panic!("same-domain lookup entity should register");
        };
        let Ok(()) = beta_target.try_add_entity(Arc::clone(&beta_entity)) else {
            panic!("cross-domain lookup entity should register");
        };

        let same_domain = EntityReference::from_uuid(alpha_uuid);
        let Some(resolved) = same_domain.get_entity(&alpha_source) else {
            panic!("entity reference should resolve in another same-domain world");
        };
        assert!(Arc::ptr_eq(&resolved, &alpha_entity));

        let unresolved_cross_domain = EntityReference::from_uuid(beta_uuid);
        assert!(unresolved_cross_domain.get_entity(&alpha_source).is_none());
        let cached_cross_domain = EntityReference::from_entity(&beta_entity);
        assert!(cached_cross_domain.get_entity(&alpha_source).is_none());
    }
}
