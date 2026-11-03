use std::sync::{Arc, Weak};

use steel_utils::locks::SyncRwLock;
use uuid::Uuid;

use crate::entity::SharedEntity;

use super::World;

/// Loaded worlds in one domain, without creating world ownership cycles.
pub(crate) struct DomainEntityDirectory {
    worlds: SyncRwLock<Vec<Weak<World>>>,
}

impl DomainEntityDirectory {
    pub(crate) const fn new() -> Self {
        Self {
            worlds: SyncRwLock::new(Vec::new()),
        }
    }

    pub(crate) fn add_world(&self, world: &Arc<World>) {
        let mut worlds = self.worlds.write();
        worlds.retain(|registered| registered.strong_count() > 0);
        if worlds
            .iter()
            .filter_map(Weak::upgrade)
            .any(|registered| Arc::ptr_eq(&registered, world))
        {
            return;
        }
        worlds.push(Arc::downgrade(world));
    }

    pub(crate) fn remove_world(&self, world: &Arc<World>) {
        self.worlds.write().retain(|registered| {
            registered
                .upgrade()
                .is_some_and(|registered| !Arc::ptr_eq(&registered, world))
        });
    }

    pub(crate) fn get_entity_by_uuid(&self, uuid: &Uuid) -> Option<SharedEntity> {
        self.worlds
            .read()
            .iter()
            .filter_map(Weak::upgrade)
            .find_map(|world| world.get_entity_by_uuid(uuid))
    }
}
