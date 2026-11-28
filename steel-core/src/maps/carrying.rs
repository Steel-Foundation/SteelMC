use std::sync::Arc;

use steel_registry::{
    REGISTRY, TaggedRegistryExt,
    data_components::vanilla_components::{MAP_DECORATIONS, MAP_ID},
    equipment::EquipmentSlot,
    item_stack::ItemStack,
    vanilla_map_decoration_types,
};
use steel_utils::{Identifier, locks::SyncMutex};

use super::{MapFrame, MapItemSavedData};
use crate::{
    behavior::ITEM_BEHAVIORS,
    entity::{Entity, ItemFrame},
    inventory::{container::Container, equipment::EntityEquipment},
    player::Player,
};

struct Carrier {
    player: Arc<Player>,
    has_map: bool,
    invisible: bool,
}

impl Carrier {
    fn capture(player: Arc<Player>, map_stack: &ItemStack) -> Self {
        let inventory = player.inventory.lock();
        let has_map = inventory
            .items()
            .iter()
            .any(|stack| stack.is(map_stack.item()) && stack.get(MAP_ID) == map_stack.get(MAP_ID));
        let tag = Identifier::vanilla_static("map_invisibility_equipment");
        let invisible = EquipmentSlot::ALL
            .into_iter()
            .filter(|slot| !matches!(slot, EquipmentSlot::MainHand | EquipmentSlot::OffHand))
            .any(|slot| {
                let item = inventory.get_ref(slot);
                REGISTRY.items.is_in_tag(item.item(), &tag)
            });
        drop(inventory);
        Self {
            player,
            has_map,
            invisible,
        }
    }
}

impl MapItemSavedData {
    /// Updates shared marker state without holding a map lock while reading
    /// player inventories; crafting may acquire these locks in the other order.
    pub fn tick_carried_by(
        data: &Arc<SyncMutex<Self>>,
        player: Arc<Player>,
        stack: &ItemStack,
        frame: Option<MapFrame>,
    ) {
        let tracked = {
            let mut data = data.lock();
            data.register_holder(&player);
            data.carried_by
                .iter()
                .filter_map(|holder| holder.player.upgrade())
                .collect::<Vec<_>>()
        };
        let carriers = tracked
            .into_iter()
            .map(|player| Carrier::capture(player, stack))
            .collect::<Vec<_>>();
        let mut data = data.lock();
        if carriers
            .iter()
            .any(|carrier| Arc::ptr_eq(&carrier.player, &player) && !carrier.has_map)
        {
            data.remove_decoration(player.gameprofile.name());
        }
        let mut index = 0;
        while index < data.carried_by.len() {
            let carrier = carriers.iter().find(|carrier| {
                data.carried_by[index]
                    .player
                    .ptr_eq(&Arc::downgrade(&carrier.player))
            });
            let Some(carrier) = carrier else {
                let holder = data.carried_by.remove(index);
                data.remove_decoration(&holder.name);
                index += 1;
                continue;
            };
            let name = carrier.player.gameprofile.name().to_owned();
            if !carrier.player.is_removed() && (frame.is_some() || carrier.has_map) {
                let world = carrier.player.get_world();
                if frame.is_none() && world.key == data.dimension && data.tracking_position {
                    let position = carrier.player.position();
                    let nether_time =
                        (data.dimension.path.as_ref() == "the_nether").then(|| world.game_time());
                    data.add_decoration(
                        &vanilla_map_decoration_types::PLAYER,
                        name.clone(),
                        position.x,
                        position.z,
                        f64::from(carrier.player.rotation().0),
                        nether_time,
                        None,
                    );
                }
            } else {
                data.carried_by.remove(index);
                data.remove_decoration(&name);
            }
            if !Arc::ptr_eq(&carrier.player, &player) && carrier.invisible {
                data.remove_decoration(&name);
            }
            index += 1;
        }
        if let Some(frame) = frame.filter(|_| data.tracking_position) {
            if let Some(previous) = data.frames.get(&frame.pos.0.to_array())
                && previous.entity_id != frame.entity_id
            {
                let key = format!("frame-{}", previous.entity_id);
                data.remove_decoration(&key);
            }
            let nether_time = (data.dimension.path.as_ref() == "the_nether")
                .then(|| player.get_world().game_time());
            data.add_decoration(
                &vanilla_map_decoration_types::FRAME,
                format!("frame-{}", frame.entity_id),
                f64::from(frame.pos.0.x),
                f64::from(frame.pos.0.z),
                f64::from(frame.rotation),
                nether_time,
                None,
            );
            if data.frames.get(&frame.pos.0.to_array()) != Some(&frame) {
                data.frames.insert(frame.pos.0.to_array(), frame);
                data.mark_dirty();
            }
        }
        if let Some(decorations) = stack.get(MAP_DECORATIONS) {
            for (key, entry) in decorations.decorations() {
                if data.decorations.iter().any(|(existing, _)| existing == key) {
                    continue;
                }
                let world = player.get_world();
                let nether_time =
                    (data.dimension.path.as_ref() == "the_nether").then(|| world.game_time());
                data.add_decoration(
                    entry.decoration_type().value(),
                    key.to_owned(),
                    entry.x(),
                    entry.z(),
                    f64::from(entry.rotation()),
                    nether_time,
                    None,
                );
            }
        }
    }

    /// Removes a frame marker when its displayed map or attachment is removed.
    pub fn removed_from_frame(&mut self, frame: &MapFrame) {
        self.remove_decoration(&format!("frame-{}", frame.entity_id));
        self.frames.remove(&frame.pos.0.to_array());
        self.mark_dirty();
    }

    pub(crate) fn synchronize_frame(frame: &dyn ItemFrame) {
        let stack = frame.item();
        if !ITEM_BEHAVIORS.get_behavior(stack.item()).is_map_item() {
            return;
        }
        let Some(world) = frame.level() else {
            return;
        };
        let Some(id) = stack.get(MAP_ID).copied() else {
            return;
        };
        let Some(data) = world.get_map_data(id) else {
            return;
        };
        let marker = MapFrame {
            pos: frame.block_position(),
            rotation: frame.direction().get_2d_data_value() * 90,
            entity_id: frame.id(),
        };
        let mut players = Vec::new();
        world.players.iter_players(|_, player| {
            players.push(Arc::clone(player));
            true
        });
        for player in players {
            Self::tick_carried_by(&data, Arc::clone(&player), &stack, Some(marker.clone()));
            let packet = data.lock().update_packet(id, &player);
            if let Some(packet) = packet {
                player.send_packet(packet);
            }
        }
    }
}
