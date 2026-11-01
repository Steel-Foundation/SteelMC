use super::chest_menu::menu_type_for_rows;
use crate::{
    block_entity::SharedBlockEntity, inventory::prelude::*,
    player::player_inventory::PlayerInventory,
};
use smallvec::SmallVec;
use steel_utils::{DowncastType, DowncastTypeKey};

/// Builds single/double chests over their original independently locked inventories.
pub(crate) fn block_chest(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    owners: SmallVec<[SharedBlockEntity; 2]>,
    containers: SmallVec<[ContainerRef; 2]>,
) -> Menu {
    let mut builder = MenuBuilder::new(menu_type_for_rows(containers.len() * 3), container_id);
    let mut sections = containers
        .iter()
        .map(|container| builder.section(container, 27));
    let Some(first) = sections.next() else {
        unreachable!("chest row count was already validated");
    };
    let chest = sections.fold(first, Section::join);
    let player = builder.player_inventory(&inventory);
    builder.route(chest, player.all(), FillDirection::Backward);
    builder.route(player.all(), chest, FillDirection::Forward);
    builder.build(BlockChestKind {
        owners,
        containers,
        opened: false,
    })
}

struct BlockChestKind {
    owners: SmallVec<[SharedBlockEntity; 2]>,
    containers: SmallVec<[ContainerRef; 2]>,
    opened: bool,
}

// SAFETY: This Steel-owned key uniquely identifies this concrete chest menu kind.
unsafe impl DowncastType for BlockChestKind {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:menu/block_chest");
}

impl MenuKind for BlockChestKind {
    fn opened_containers(&self) -> SmallVec<[ContainerId; 2]> {
        self.containers
            .iter()
            .map(ContainerRef::container_id)
            .collect()
    }
    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        self.containers
            .iter()
            .all(|container| container.still_valid(player))
    }
    fn on_open(
        &mut self,
        _behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        player: &Player,
    ) {
        self.opened = true;
        guard.run_unlocked(|| {
            for owner in &self.owners {
                owner.start_open(player);
            }
        });
    }
    fn removed(&mut self, _behavior: &mut MenuBehavior, player: &Player) {
        if !self.opened {
            return;
        }
        self.opened = false;
        for owner in &self.owners {
            owner.stop_open(player);
        }
    }
}
