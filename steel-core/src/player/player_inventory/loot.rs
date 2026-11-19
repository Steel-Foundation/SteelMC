use std::{ptr, sync::Arc};

use crate::{
    block_entity::SharedBlockEntity,
    entity::Entity,
    loot::OperationError,
    player::{Player, connection::NetworkConnection},
};

impl Player {
    /// Prepares every half at the original interaction, retaining event snapshots
    /// and stream request order. A later menu request supersedes a pending open.
    pub(crate) fn open_loot_container_menu(
        &self,
        owners: Vec<SharedBlockEntity>,
        ready: impl FnOnce(&Player) + Send + 'static,
    ) {
        let mut pending = None;
        for owner in &owners {
            let Some(randomizable) = owner.as_randomizable_container() else {
                continue;
            };
            match randomizable.unpack_loot_table(Some(self)) {
                Ok(()) => (),
                Err(OperationError::Pending(completion)) => {
                    pending.get_or_insert(completion);
                }
                Err(error) => {
                    log::error!(
                        "Could not prepare container menu at {:?}: {error}",
                        owner.get_block_pos()
                    );
                    return;
                }
            }
        }
        let Some(pending) = pending else {
            ready(self);
            return;
        };
        let world = self.get_world();
        let Some(player) = world
            .players
            .get_by_entity_id(self.id())
            .filter(|player| ptr::eq(player.as_ref(), self))
        else {
            log::error!("Cannot retain a loot menu for an unregistered player");
            return;
        };
        let request = Arc::new(());
        self.open_menu.lock().pending_open = Some(Arc::clone(&request));
        let residence = self.domain_residence_token();
        pending.on_complete(move |result| {
            if result.is_err()
                || player.is_removed()
                || player.connection.closed()
                || !player.is_domain_residence_current(residence)
                || !Arc::ptr_eq(&world, &player.get_world())
                || !world
                    .players
                    .get_by_entity_id(player.id())
                    .is_some_and(|current| Arc::ptr_eq(&current, &player))
                || !player
                    .open_menu
                    .lock()
                    .pending_open
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &request))
                || owners.iter().any(|owner| {
                    !owner.base().is_valid_container_for(&player)
                        || !world
                            .get_block_entity(owner.get_block_pos())
                            .is_some_and(|current| Arc::ptr_eq(&current, owner))
                })
            {
                return;
            }
            player.open_loot_container_menu(owners, ready);
        });
    }
}
