//! Reloadable advancement tree.

use std::sync::Arc;

use steel_registry::{REGISTRY, advancement::registry::FrozenAdvancementRegistry};

use super::{PreparedResource, ReloadableResource};
use crate::server::Server;

pub(super) struct AdvancementsResource;

impl ReloadableResource for AdvancementsResource {
    fn name(&self) -> &'static str {
        "advancements"
    }

    fn reset(&self) {
        REGISTRY.advancements.reset_to_vanilla();
    }

    fn prepare(&self) -> Result<Box<dyn PreparedResource>, String> {
        Ok(Box::new(PreparedAdvancements {
            tree: REGISTRY.advancements.prepare_tree(),
        }))
    }
}

struct PreparedAdvancements {
    tree: FrozenAdvancementRegistry,
}

impl PreparedResource for PreparedAdvancements {
    fn apply(self: Box<Self>, server: &Server) -> Result<(), String> {
        REGISTRY.advancements.replace_tree(self.tree);
        let tree = REGISTRY.advancements.snapshot();
        for player in server.get_players() {
            player.advancements.lock().sync(Arc::clone(&tree));
        }
        Ok(())
    }
}
