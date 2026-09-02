//! Runtime tracking and client synchronization for carried filled maps.

use std::{
    ptr,
    sync::{Arc, Weak},
};

use glam::DVec3;
use rustc_hash::{FxHashMap, FxHashSet};
use steel_protocol::packets::game::{CMapItemData, MapDecoration, MapPatch};
use steel_registry::{
    RegistryReference,
    data_components::{
        MapDecorationEntry, MapDecorations, MapId,
        vanilla_components::{MAP_DECORATIONS, MAP_ID},
    },
    item_stack::ItemStack,
    vanilla_item_tags::ItemTag,
    vanilla_items, vanilla_map_decoration_types,
};
use steel_utils::{ChunkPos, Identifier};
use uuid::Uuid;

use crate::{
    chunk::{
        chunk_map::ChunkMap,
        chunk_request::{ChunkRequest, ChunkRequestHandle, ChunkRequestState, ChunkTicketKind},
        status::ChunkStatus,
    },
    entity::Entity as _,
    inventory::{
        container::Container as _,
        equipment::{EntityEquipment as _, EquipmentSlot},
    },
    player::Player,
    server::worlds::NETHER_WORLD_NAME,
    world::World,
};

use super::{
    MapDataStore, MapItemSavedData,
    terrain::{MapDirtyBounds, sampling_chunks, update_map},
};

/// One filled-map stack observed in a player's inventory.
pub(crate) struct CarriedMap {
    id: i32,
    decorations: MapDecorations,
}

#[derive(Default)]
pub(super) struct MapTrackingState {
    maps: FxHashMap<i32, TrackedMap>,
    player_maps: FxHashMap<Uuid, FxHashSet<i32>>,
}

#[derive(Default)]
struct TrackedMap {
    holders: Vec<HoldingPlayer>,
    static_entries: Vec<(String, MapDecorationEntry)>,
    decorations: Vec<(String, MapDecoration)>,
}

struct HoldingPlayer {
    uuid: Uuid,
    name: String,
    player: Weak<Player>,
    dirty_data: Option<MapDirtyBounds>,
    dirty_decorations: bool,
    decoration_tick: i32,
    map_step: i32,
    terrain_request: Option<ChunkRequestHandle>,
    sent_decorations: Vec<MapDecoration>,
}

struct HolderCandidate {
    uuid: Uuid,
    player: Weak<Player>,
}

struct HolderSnapshot {
    uuid: Uuid,
    name: String,
    world: Identifier,
    position: DVec3,
    yaw: f32,
    game_time: i64,
    invisible_on_maps: bool,
}

impl CarriedMap {
    fn map_id(item: &ItemStack) -> Option<i32> {
        if !item.is(&vanilla_items::FILLED_MAP) {
            return None;
        }
        let id = item.get(MAP_ID)?.id();
        (id >= 0).then_some(id)
    }

    /// Extracts the map identity and static decorations from a filled-map stack.
    pub(crate) fn from_item(item: &ItemStack) -> Option<Self> {
        let id = Self::map_id(item)?;
        Some(Self {
            id,
            decorations: item
                .get(MAP_DECORATIONS)
                .cloned()
                .unwrap_or(MapDecorations::EMPTY),
        })
    }

    pub(crate) fn held_id(item: &ItemStack) -> Option<i32> {
        Self::map_id(item)
    }
}

impl MapDataStore {
    /// Removes all runtime map state owned by a disconnected or domain-switching player.
    pub(crate) fn remove_player_tracking(&self, uuid: Uuid) {
        self.tracking.lock().remove_player(uuid);
    }

    /// Releases terrain-loading tickets while preserving same-domain map synchronization state.
    pub(crate) fn clear_player_terrain_requests(&self, uuid: Uuid) {
        self.tracking.lock().clear_terrain_requests(uuid);
    }

    /// Advances Vanilla's carried-map tracking and returns packets for `player`.
    pub(crate) fn synchronize_player(
        &self,
        player: &Arc<Player>,
        carried: &[CarriedMap],
        held: &[i32],
    ) -> Vec<CMapItemData> {
        let world = player.get_world();
        let requested_chunks = self.held_map_sampling_chunks(&world, player.position(), held);
        let requested_map_ids = requested_chunks.keys().copied().collect::<FxHashSet<_>>();
        let valid_slots = self.valid_map_slots(carried);
        let mut seen = FxHashSet::default();
        let unique_order = valid_slots
            .iter()
            .copied()
            .filter(|id| seen.insert(*id))
            .collect::<Vec<_>>();
        let (candidate_groups, map_steps) = {
            let mut tracking = self.tracking.lock();
            tracking.update_player(player, carried, &unique_order);
            let candidates = unique_order
                .iter()
                .map(|&id| (id, tracking.holder_candidates(id)))
                .collect::<Vec<_>>();
            tracking.retain_terrain_requests(player.uuid(), &requested_map_ids);
            let steps = held
                .iter()
                .filter_map(|&id| {
                    let chunks = requested_chunks.get(&id)?;
                    tracking
                        .advance_step(id, player.uuid(), &world.chunk_map, chunks)
                        .map(|step| (id, step))
                })
                .collect::<Vec<_>>();
            (candidates, steps)
        };
        self.update_held_maps(player, &map_steps);

        let mut snapshots = FxHashMap::default();
        let mut stale_holders = Vec::new();
        for (id, candidates) in candidate_groups {
            let mut available = Vec::with_capacity(candidates.len());
            for candidate in candidates {
                if let Some(snapshot) = HolderSnapshot::capture(self, id, &candidate) {
                    available.push(snapshot);
                } else {
                    stale_holders.push((id, candidate.uuid));
                }
            }
            snapshots.insert(id, available);
        }

        let map_state = self.state.read();
        let mut tracking = self.tracking.lock();
        for (id, uuid) in stale_holders {
            tracking.remove_holder(id, uuid);
        }

        let player_id = player.uuid();
        let mut decorations_by_id = FxHashMap::default();
        for &id in &unique_order {
            let Some(map) = map_state.maps.get(&id) else {
                continue;
            };
            let Some(tracked) = tracking.maps.get_mut(&id) else {
                continue;
            };
            let Some(holder_snapshots) = snapshots.get(&id) else {
                continue;
            };
            decorations_by_id.insert(
                id,
                tracked.decorations_for(player_id, map, holder_snapshots),
            );
        }

        let mut packets = Vec::new();
        for id in valid_slots {
            let Some(map) = map_state.maps.get(&id) else {
                continue;
            };
            let Some(tracked) = tracking.maps.get_mut(&id) else {
                continue;
            };
            let Some(decorations) = decorations_by_id.get(&id) else {
                continue;
            };
            let Some(holder) = tracked
                .holders
                .iter_mut()
                .find(|holder| holder.uuid == player_id)
            else {
                continue;
            };
            if let Some(packet) = holder.next_update_packet(id, map, decorations) {
                packets.push(packet);
            }
        }
        packets
    }

    fn held_map_sampling_chunks(
        &self,
        world: &World,
        player_position: DVec3,
        held: &[i32],
    ) -> FxHashMap<i32, Vec<ChunkPos>> {
        if !ptr::eq(world.map_data().as_ref(), self) {
            return FxHashMap::default();
        }
        let state = self.state.read();
        let mut requests = FxHashMap::default();
        for &id in held {
            if requests.contains_key(&id) {
                continue;
            }
            let Some(map) = state.maps.get(&id) else {
                continue;
            };
            let chunks = sampling_chunks(world, player_position, map);
            if !chunks.is_empty() {
                requests.insert(id, chunks);
            }
        }
        requests
    }

    fn update_held_maps(&self, player: &Arc<Player>, map_steps: &[(i32, i32)]) {
        if map_steps.is_empty() {
            return;
        }
        let world = player.get_world();
        if !ptr::eq(world.map_data().as_ref(), self) {
            return;
        }
        let player_position = player.position();
        let mut changed_maps: Vec<(i32, MapDirtyBounds)> = Vec::new();
        {
            let mut state = self.state.write();
            for &(id, step) in map_steps {
                let Some(map) = state.maps.get_mut(&id) else {
                    continue;
                };
                let Some(bounds) = update_map(&world, player_position, step, map) else {
                    continue;
                };
                if let Some((_, existing)) = changed_maps
                    .iter_mut()
                    .find(|(existing_id, _)| *existing_id == id)
                {
                    existing.merge(bounds);
                } else {
                    changed_maps.push((id, bounds));
                }
            }
            if !changed_maps.is_empty() {
                state.revision = state.revision.wrapping_add(1);
            }
        }
        if changed_maps.is_empty() {
            return;
        }
        let mut tracking = self.tracking.lock();
        for (id, bounds) in changed_maps {
            if let Some(map) = tracking.maps.get_mut(&id) {
                for holder in &mut map.holders {
                    holder.mark_colors_dirty(bounds);
                }
            }
        }
    }

    fn valid_map_slots(&self, carried: &[CarriedMap]) -> Vec<i32> {
        let state = self.state.read();
        carried
            .iter()
            .filter_map(|map| state.maps.contains_key(&map.id).then_some(map.id))
            .collect()
    }
}

impl MapTrackingState {
    fn remove_player(&mut self, uuid: Uuid) {
        let Some(map_ids) = self.player_maps.remove(&uuid) else {
            return;
        };
        for id in map_ids {
            if let Some(map) = self.maps.get_mut(&id) {
                map.remove_holder(uuid);
            }
        }
    }

    fn clear_terrain_requests(&mut self, uuid: Uuid) {
        let (player_maps, maps) = (&self.player_maps, &mut self.maps);
        let Some(map_ids) = player_maps.get(&uuid) else {
            return;
        };
        for id in map_ids {
            let Some(map) = maps.get_mut(id) else {
                continue;
            };
            if let Some(holder) = map.holders.iter_mut().find(|holder| holder.uuid == uuid) {
                holder.terrain_request = None;
            }
        }
    }

    fn update_player(&mut self, player: &Arc<Player>, carried: &[CarriedMap], valid_order: &[i32]) {
        let uuid = player.uuid();
        let current = valid_order.iter().copied().collect::<FxHashSet<_>>();
        let previous = self.player_maps.get(&uuid).cloned().unwrap_or_default();
        for id in previous.difference(&current).copied().collect::<Vec<_>>() {
            self.remove_holder(id, uuid);
        }

        for &id in valid_order {
            let tracked = self.maps.entry(id).or_default();
            if let Some(holder) = tracked
                .holders
                .iter_mut()
                .find(|holder| holder.uuid == uuid)
            {
                let current_player = Arc::downgrade(player);
                if holder.player.ptr_eq(&current_player) {
                    holder.player = current_player;
                } else {
                    *holder = HoldingPlayer::new(player);
                }
            } else {
                tracked.holders.push(HoldingPlayer::new(player));
            }
            for map in carried.iter().filter(|map| map.id == id) {
                tracked.add_static_decorations(&map.decorations);
            }
        }

        if current.is_empty() {
            self.player_maps.remove(&uuid);
        } else {
            self.player_maps.insert(uuid, current);
        }
    }

    fn holder_candidates(&self, id: i32) -> Vec<HolderCandidate> {
        self.maps.get(&id).map_or_else(Vec::new, |tracked| {
            tracked
                .holders
                .iter()
                .map(|holder| HolderCandidate {
                    uuid: holder.uuid,
                    player: holder.player.clone(),
                })
                .collect()
        })
    }

    fn remove_holder(&mut self, id: i32, uuid: Uuid) {
        if let Some(tracked) = self.maps.get_mut(&id) {
            tracked.remove_holder(uuid);
        }
        let remove_index = self.player_maps.get_mut(&uuid).is_some_and(|maps| {
            maps.remove(&id);
            maps.is_empty()
        });
        if remove_index {
            self.player_maps.remove(&uuid);
        }
    }

    fn retain_terrain_requests(&mut self, uuid: Uuid, requested_maps: &FxHashSet<i32>) {
        for (&id, map) in &mut self.maps {
            if requested_maps.contains(&id) {
                continue;
            }
            if let Some(holder) = map.holders.iter_mut().find(|holder| holder.uuid == uuid) {
                holder.terrain_request = None;
            }
        }
    }

    fn advance_step(
        &mut self,
        id: i32,
        uuid: Uuid,
        chunk_map: &Arc<ChunkMap>,
        sampling_chunks: &[ChunkPos],
    ) -> Option<i32> {
        let holder = self
            .maps
            .get_mut(&id)?
            .holders
            .iter_mut()
            .find(|holder| holder.uuid == uuid)?;
        let request_matches = holder
            .terrain_request
            .as_ref()
            .is_some_and(|request| request.positions() == sampling_chunks);
        if !request_matches {
            holder.terrain_request = Some(chunk_map.request_chunks(ChunkRequest {
                status: ChunkStatus::Full,
                positions: sampling_chunks.to_vec(),
                ticket_kind: ChunkTicketKind::Map,
            }));
        }
        if holder.terrain_request.as_ref()?.poll() != ChunkRequestState::Ready {
            return None;
        }
        holder.map_step = holder.map_step.wrapping_add(1);
        Some(holder.map_step)
    }
}

impl TrackedMap {
    fn add_static_decorations(&mut self, decorations: &MapDecorations) {
        for (id, decoration) in decorations.decorations() {
            if self
                .static_entries
                .iter()
                .all(|(existing, _)| existing != id)
            {
                self.static_entries.push((id.clone(), decoration.clone()));
            }
        }
    }

    fn decorations_for(
        &mut self,
        recipient: Uuid,
        map: &MapItemSavedData,
        holders: &[HolderSnapshot],
    ) -> Vec<MapDecoration> {
        for holder in holders {
            if holder.world == map.world && map.tracking_position {
                if let Some(decoration) = player_decoration(map, holder) {
                    insert_decoration(&mut self.decorations, holder.name.clone(), decoration);
                } else {
                    remove_decoration(&mut self.decorations, &holder.name);
                }
            }
            if holder.uuid != recipient && holder.invisible_on_maps {
                remove_decoration(&mut self.decorations, &holder.name);
            }
        }

        let recipient_game_time = holders
            .iter()
            .find(|holder| holder.uuid == recipient)
            .map_or(0, |holder| holder.game_time);
        for (id, entry) in &self.static_entries {
            if self.decorations.iter().any(|(existing, _)| existing == id) {
                continue;
            }
            if let Some(decoration) = static_decoration(map, entry, recipient_game_time) {
                self.decorations.push((id.clone(), decoration));
            }
        }
        self.decorations
            .iter()
            .map(|(_, decoration)| decoration)
            .cloned()
            .collect()
    }

    fn remove_holder(&mut self, uuid: Uuid) {
        let Some(index) = self.holders.iter().position(|holder| holder.uuid == uuid) else {
            return;
        };
        let holder = self.holders.remove(index);
        remove_decoration(&mut self.decorations, &holder.name);
    }
}

impl HoldingPlayer {
    fn new(player: &Arc<Player>) -> Self {
        Self {
            uuid: player.uuid(),
            name: player.gameprofile.name.clone(),
            player: Arc::downgrade(player),
            dirty_data: Some(MapDirtyBounds::FULL),
            dirty_decorations: true,
            decoration_tick: 0,
            map_step: 0,
            terrain_request: None,
            sent_decorations: Vec::new(),
        }
    }

    fn mark_colors_dirty(&mut self, bounds: MapDirtyBounds) {
        if let Some(dirty) = &mut self.dirty_data {
            dirty.merge(bounds);
        } else {
            self.dirty_data = Some(bounds);
        }
    }

    fn next_update_packet(
        &mut self,
        id: i32,
        map: &MapItemSavedData,
        decorations: &[MapDecoration],
    ) -> Option<CMapItemData> {
        if decorations != self.sent_decorations {
            self.dirty_decorations = true;
        }
        let color_patch = self.dirty_data.take().map(|bounds| {
            let width = bounds.max_x - bounds.min_x + 1;
            let height = bounds.max_y - bounds.min_y + 1;
            let mut colors = Vec::with_capacity(usize::from(width) * usize::from(height));
            for y in bounds.min_y..=bounds.max_y {
                let start = usize::from(bounds.min_x) + usize::from(y) * 128;
                colors.extend_from_slice(&map.colors[start..start + usize::from(width)]);
            }
            MapPatch {
                start_x: bounds.min_x,
                start_y: bounds.min_y,
                width,
                height,
                colors,
            }
        });
        let packet_decorations = if self.dirty_decorations {
            let send = self.decoration_tick % 5 == 0;
            self.decoration_tick = self.decoration_tick.wrapping_add(1);
            send.then(|| {
                self.dirty_decorations = false;
                let decorations = decorations.to_vec();
                self.sent_decorations.clone_from(&decorations);
                decorations
            })
        } else {
            None
        };
        if color_patch.is_none() && packet_decorations.is_none() {
            return None;
        }
        Some(CMapItemData {
            map_id: MapId::new(id),
            scale: map.scale,
            locked: map.locked,
            decorations: packet_decorations,
            color_patch,
        })
    }
}

impl HolderSnapshot {
    fn capture(store: &MapDataStore, map_id: i32, candidate: &HolderCandidate) -> Option<Self> {
        let player = candidate.player.upgrade()?;
        if player.is_removed() {
            return None;
        }
        let world = player.get_world();
        if !ptr::eq(world.map_data().as_ref(), store) {
            return None;
        }
        let invisible_on_maps = {
            let inventory = player.inventory.lock();
            let carries_map = inventory.items().iter().any(|item| {
                item.is(&vanilla_items::FILLED_MAP)
                    && item.get(MAP_ID).is_some_and(|id| id.id() == map_id)
            });
            if !carries_map {
                return None;
            }
            EquipmentSlot::ALL.into_iter().any(|slot| {
                !matches!(slot, EquipmentSlot::MainHand | EquipmentSlot::OffHand)
                    && inventory
                        .get_ref(slot)
                        .item()
                        .has_tag(&ItemTag::MAP_INVISIBILITY_EQUIPMENT)
            })
        };
        Some(Self {
            uuid: candidate.uuid,
            name: player.gameprofile.name.clone(),
            world: world.key.clone(),
            position: player.position(),
            yaw: player.rotation().0,
            game_time: world.game_time(),
            invisible_on_maps,
        })
    }
}

fn player_decoration(map: &MapItemSavedData, holder: &HolderSnapshot) -> Option<MapDecoration> {
    let (x_delta, y_delta) = map_deltas(map, holder.position.x, holder.position.z);
    let x = clamp_map_coordinate(x_delta);
    let y = clamp_map_coordinate(y_delta);
    if inside_map(x_delta, y_delta) {
        return Some(MapDecoration::new(
            RegistryReference::new(&vanilla_map_decoration_types::PLAYER),
            x,
            y,
            map_rotation(map, f64::from(holder.yaw), holder.game_time),
            None,
        ));
    }
    let decoration_type = if x_delta.abs() < 320.0 && y_delta.abs() < 320.0 {
        &vanilla_map_decoration_types::PLAYER_OFF_MAP
    } else if map.unlimited_tracking {
        &vanilla_map_decoration_types::PLAYER_OFF_LIMITS
    } else {
        return None;
    };
    Some(MapDecoration::new(
        RegistryReference::new(decoration_type),
        x,
        y,
        0,
        None,
    ))
}

fn static_decoration(
    map: &MapItemSavedData,
    entry: &MapDecorationEntry,
    game_time: i64,
) -> Option<MapDecoration> {
    let (x_delta, y_delta) = map_deltas(map, entry.x(), entry.z());
    let decoration_type = entry.decoration_type().value();
    if decoration_type.key == vanilla_map_decoration_types::PLAYER.key {
        let snapshot = HolderSnapshot {
            uuid: Uuid::nil(),
            name: String::new(),
            world: map.world.clone(),
            position: DVec3::new(entry.x(), 0.0, entry.z()),
            yaw: entry.rotation(),
            game_time,
            invisible_on_maps: false,
        };
        return player_decoration(map, &snapshot);
    }
    if !inside_map(x_delta, y_delta) && !map.unlimited_tracking {
        return None;
    }
    Some(MapDecoration::new(
        RegistryReference::new(decoration_type),
        clamp_map_coordinate(x_delta),
        clamp_map_coordinate(y_delta),
        map_rotation(map, f64::from(entry.rotation()), game_time),
        None,
    ))
}

fn map_deltas(map: &MapItemSavedData, x: f64, z: f64) -> (f32, f32) {
    let scale = 1_i32.wrapping_shl(u32::from(map.scale as u8) & 31);
    (
        (x - f64::from(map.center_x)) as f32 / scale as f32,
        (z - f64::from(map.center_z)) as f32 / scale as f32,
    )
}

fn map_rotation(map: &MapItemSavedData, yaw: f64, game_time: i64) -> i8 {
    // TODO: Revisit this when custom worlds have an explicit Vanilla level-key identity.
    if map.world.path.as_ref() == NETHER_WORLD_NAME {
        let time = (game_time / 10) as i32;
        return time
            .wrapping_mul(time)
            .wrapping_mul(34_187_121)
            .wrapping_add(time.wrapping_mul(121))
            .wrapping_shr(15) as i8
            & 15;
    }
    let adjusted = if yaw < 0.0 { yaw - 8.0 } else { yaw + 8.0 };
    (adjusted * 16.0 / 360.0) as i32 as i8
}

fn clamp_map_coordinate(delta: f32) -> i8 {
    if delta <= -63.0 {
        -128
    } else if delta >= 63.0 {
        127
    } else {
        (delta * 2.0 + 0.5) as i8
    }
}

fn inside_map(x_delta: f32, y_delta: f32) -> bool {
    (-63.0..=63.0).contains(&x_delta) && (-63.0..=63.0).contains(&y_delta)
}

fn insert_decoration(
    decorations: &mut Vec<(String, MapDecoration)>,
    id: String,
    decoration: MapDecoration,
) {
    if let Some((_, existing)) = decorations.iter_mut().find(|(existing, _)| *existing == id) {
        *existing = decoration;
    } else {
        decorations.push((id, decoration));
    }
}

fn remove_decoration(decorations: &mut Vec<(String, MapDecoration)>, id: &str) {
    if let Some(index) = decorations.iter().position(|(existing, _)| existing == id) {
        decorations.remove(index);
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use futures::executor;
    use glam::DVec3;
    use steel_registry::{
        RegistryReference,
        data_components::{
            MapDecorationEntry, MapDecorations, vanilla_components::MAP_DECORATIONS,
        },
        map_color::{MapBrightness, MapColor},
        test_support::init_test_registry,
        vanilla_dimension_types, vanilla_items, vanilla_map_decoration_types,
    };
    use steel_utils::{BlockPos, ChunkPos};
    use uuid::Uuid;

    use crate::{
        map::{MAP_COLOR_COUNT, NewMapData},
        player::ResetReason,
        test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk},
    };

    use super::*;

    #[test]
    fn first_carried_map_update_contains_full_colors_and_decorations() {
        init_test_registry();
        let world = fresh_test_world("map_sync");
        let store = world.map_data();
        let mut colors = vec![0; MAP_COLOR_COUNT];
        colors[123] = 42;
        let map_id = store
            .create_map(NewMapData {
                origin: BlockPos::new(0, 64, 0),
                scale: 1,
                tracking_position: true,
                unlimited_tracking: true,
                world: world.key.clone(),
                dimension_type: vanilla_dimension_types::OVERWORLD.key.clone(),
                colors: colors.clone(),
            })
            .expect("map allocation should succeed");
        let player =
            TestPlayerBuilder::new(Arc::clone(&world), "Mapper", 1)
                .uuid(Uuid::from_u128(1))
                .build();
        player
            .base()
            .set_position_local(DVec3::new(8.0, 64.0, -8.0));
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));

        let mut map = ItemStack::new(&vanilla_items::FILLED_MAP);
        map.set(MAP_ID, map_id);
        let target = MapDecorationEntry::new(
            RegistryReference::new(&vanilla_map_decoration_types::RED_X),
            32.0,
            -16.0,
            180.0,
        );
        map.set(
            MAP_DECORATIONS,
            MapDecorations::EMPTY.with_decoration("+".to_owned(), target),
        );
        player.inventory.lock().items_mut()[0] = map;
        let carried = player
            .inventory
            .lock()
            .items()
            .iter()
            .filter_map(CarriedMap::from_item)
            .collect::<Vec<_>>();

        let packets = store.synchronize_player(&player, &carried, &[]);
        assert_eq!(packets.len(), 1);
        let packet = &packets[0];
        assert_eq!(packet.map_id, map_id);
        assert_eq!(packet.scale, 1);
        let Some(decorations) = &packet.decorations else {
            panic!("first map update should contain decorations");
        };
        assert_eq!(decorations.len(), 2);
        assert_eq!(
            decorations[0].decoration_type.value().key,
            vanilla_map_decoration_types::PLAYER.key
        );
        assert_eq!((decorations[0].x, decorations[0].y), (-55, -71));
        assert_eq!(
            decorations[1].decoration_type.value().key,
            vanilla_map_decoration_types::RED_X.key
        );
        assert_eq!(
            (decorations[1].x, decorations[1].y, decorations[1].rotation),
            (-31, -79, 8)
        );
        let Some(patch) = &packet.color_patch else {
            panic!("first map update should contain a full color patch");
        };
        assert_eq!((patch.width, patch.height), (128, 128));
        assert_eq!(patch.colors, colors);
        assert!(store.synchronize_player(&player, &carried, &[]).is_empty());
    }

    #[test]
    fn reconnecting_holder_receives_fresh_full_color_patch() {
        init_test_registry();
        let world = fresh_test_world("map_reconnect_sync");
        let store = world.map_data();
        let map_id = store
            .create_map(NewMapData {
                origin: BlockPos::new(0, 64, 0),
                scale: 1,
                tracking_position: true,
                unlimited_tracking: true,
                world: world.key.clone(),
                dimension_type: vanilla_dimension_types::OVERWORLD.key.clone(),
                colors: vec![MapColor::STONE.packed_id(MapBrightness::Normal); MAP_COLOR_COUNT],
            })
            .expect("map allocation should succeed");
        let uuid = Uuid::from_u128(11);
        let first = TestPlayerBuilder::new(Arc::clone(&world), "Reconnect", 11)
            .uuid(uuid)
            .build();
        assert!(world.add_player(Arc::clone(&first), ResetReason::InitialJoin));

        let mut map = ItemStack::new(&vanilla_items::FILLED_MAP);
        map.set(MAP_ID, map_id);
        first.inventory.lock().items_mut()[9] = map.clone();
        let carried = first
            .inventory
            .lock()
            .items()
            .iter()
            .filter_map(CarriedMap::from_item)
            .collect::<Vec<_>>();
        assert_eq!(store.synchronize_player(&first, &carried, &[]).len(), 1);

        world.remove_player_for_world_change(&first);
        let second = TestPlayerBuilder::new(Arc::clone(&world), "Reconnect", 12)
            .uuid(uuid)
            .build();
        second.inventory.lock().items_mut()[9] = map;
        assert!(world.add_player(Arc::clone(&second), ResetReason::InitialJoin));
        let carried = second
            .inventory
            .lock()
            .items()
            .iter()
            .filter_map(CarriedMap::from_item)
            .collect::<Vec<_>>();

        let packets = store.synchronize_player(&second, &carried, &[]);
        assert_eq!(packets.len(), 1);
        let patch = packets[0]
            .color_patch
            .as_ref()
            .expect("a new player session needs the complete map colors");
        assert_eq!((patch.width, patch.height), (128, 128));
    }

    #[test]
    fn held_map_updates_full_air_chunks_and_requests_unloaded_sampling_chunks() {
        init_test_registry();
        let world = fresh_test_world("held_map_terrain");
        let surface = BlockPos::new(0, 64, 0);

        let store = world.map_data();
        let map_id = store
            .create_map(NewMapData {
                origin: surface,
                scale: 0,
                tracking_position: true,
                unlimited_tracking: true,
                world: world.key.clone(),
                dimension_type: vanilla_dimension_types::OVERWORLD.key.clone(),
                colors: vec![0; MAP_COLOR_COUNT],
            })
            .expect("map allocation should succeed");
        let player =
            TestPlayerBuilder::new(Arc::clone(&world), "Surveyor", 13)
                .uuid(Uuid::from_u128(13))
                .build();
        player.base().set_position_local(DVec3::new(0.0, 64.0, 0.0));
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let mut map = ItemStack::new(&vanilla_items::FILLED_MAP);
        map.set(MAP_ID, map_id);
        player.inventory.lock().items_mut()[0] = map;
        let carried = player
            .inventory
            .lock()
            .items()
            .iter()
            .filter_map(CarriedMap::from_item)
            .collect::<Vec<_>>();

        let _ = store.synchronize_player(&player, &carried, &[map_id.id()]);
        let requested_positions = {
            let tracking = store.tracking.lock();
            let holder = &tracking.maps[&map_id.id()].holders[0];
            let request = holder
                .terrain_request
                .as_ref()
                .expect("a held map should retain tickets for its complete sampling area");
            assert_eq!(request.ticket_kind(), Some(ChunkTicketKind::Map));
            assert!(
                request.positions().contains(&ChunkPos::new(-4, -5)),
                "the map should request an unloaded edge chunk sampled by Vanilla"
            );
            assert_eq!(
                request.poll(),
                ChunkRequestState::Pending {
                    ready: 0,
                    total: 72
                }
            );
            request.positions().to_vec()
        };
        for chunk in requested_positions {
            insert_ready_full_chunk(&world, chunk);
        }
        for _ in 0..100 {
            world.chunk_map.advance_scheduling();
            let ready = {
                let tracking = store.tracking.lock();
                tracking.maps[&map_id.id()].holders[0]
                    .terrain_request
                    .as_ref()
                    .is_some_and(|request| request.poll() == ChunkRequestState::Ready)
            };
            if ready {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        {
            let tracking = store.tracking.lock();
            let request = tracking.maps[&map_id.id()].holders[0]
                .terrain_request
                .as_ref()
                .expect("terrain request should remain active");
            assert_eq!(request.poll(), ChunkRequestState::Ready);
        }

        for _ in 0..16 {
            let _ = store.synchronize_player(&player, &carried, &[map_id.id()]);
        }

        let state = store.state.read();
        let map = &state.maps[&map_id.id()];
        assert_eq!(
            map.colors[64 + 64 * 128],
            MapColor::STONE.packed_id(MapBrightness::Normal)
        );
        drop(state);

        let _ = world.detach_player_for_disconnect(player);
        let tracking = store.tracking.lock();
        assert!(tracking.maps[&map_id.id()].holders.is_empty());
        drop(tracking);

        world.chunk_map.stop_generation_refill_loop();
        world.chunk_map.task_tracker.close();
        executor::block_on(world.chunk_map.task_tracker.wait());
    }

    #[test]
    fn moving_player_decoration_is_sent_on_vanilla_fifth_dirty_check() {
        init_test_registry();
        let world = fresh_test_world("map_sync_rate");
        let store = world.map_data();
        let map_id = store
            .create_map(NewMapData {
                origin: BlockPos::new(0, 64, 0),
                scale: 0,
                tracking_position: true,
                unlimited_tracking: true,
                world: world.key.clone(),
                dimension_type: vanilla_dimension_types::OVERWORLD.key.clone(),
                colors: vec![0; MAP_COLOR_COUNT],
            })
            .expect("map allocation should succeed");
        let player =
            TestPlayerBuilder::new(Arc::clone(&world), "Walker", 2)
                .uuid(Uuid::from_u128(2))
                .build();
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let mut map = ItemStack::new(&vanilla_items::FILLED_MAP);
        map.set(MAP_ID, map_id);
        player.inventory.lock().items_mut()[0] = map;
        let carried = player
            .inventory
            .lock()
            .items()
            .iter()
            .filter_map(CarriedMap::from_item)
            .collect::<Vec<_>>();

        assert_eq!(store.synchronize_player(&player, &carried, &[]).len(), 1);
        player
            .try_set_position(DVec3::new(10.0, 64.0, 0.0))
            .expect("registered test player should move through the world entity manager");
        for _ in 0..4 {
            assert!(store.synchronize_player(&player, &carried, &[]).is_empty());
        }
        let packets = store.synchronize_player(&player, &carried, &[]);
        assert_eq!(packets.len(), 1);
        assert!(packets[0].color_patch.is_none());
        assert!(packets[0].decorations.is_some());
    }

    #[test]
    fn duplicate_map_slots_each_advance_the_decoration_throttle() {
        init_test_registry();
        let world = fresh_test_world("map_sync_duplicate_cadence");
        let store = world.map_data();
        let map_id = store
            .create_map(NewMapData {
                origin: BlockPos::new(0, 64, 0),
                scale: 0,
                tracking_position: true,
                unlimited_tracking: true,
                world: world.key.clone(),
                dimension_type: vanilla_dimension_types::OVERWORLD.key.clone(),
                colors: vec![0; MAP_COLOR_COUNT],
            })
            .expect("map allocation should succeed");
        let player =
            TestPlayerBuilder::new(Arc::clone(&world), "Copies", 5)
                .uuid(Uuid::from_u128(5))
                .build();
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        for slot in 0..2 {
            let mut map = ItemStack::new(&vanilla_items::FILLED_MAP);
            map.set(MAP_ID, map_id);
            player.inventory.lock().items_mut()[slot] = map;
        }
        let carried = player
            .inventory
            .lock()
            .items()
            .iter()
            .filter_map(CarriedMap::from_item)
            .collect::<Vec<_>>();

        assert_eq!(store.synchronize_player(&player, &carried, &[]).len(), 1);
        player
            .try_set_position(DVec3::new(10.0, 64.0, 0.0))
            .expect("registered test player should move through the world entity manager");
        for _ in 0..2 {
            assert!(store.synchronize_player(&player, &carried, &[]).is_empty());
        }
        let packets = store.synchronize_player(&player, &carried, &[]);
        assert_eq!(packets.len(), 1);
        assert!(packets[0].decorations.is_some());
    }

    #[test]
    fn later_holders_are_inserted_after_existing_static_decorations() {
        init_test_registry();
        let world = fresh_test_world("map_sync_order");
        let store = world.map_data();
        let map_id = store
            .create_map(NewMapData {
                origin: BlockPos::new(0, 64, 0),
                scale: 0,
                tracking_position: true,
                unlimited_tracking: true,
                world: world.key.clone(),
                dimension_type: vanilla_dimension_types::OVERWORLD.key.clone(),
                colors: vec![0; MAP_COLOR_COUNT],
            })
            .expect("map allocation should succeed");
        let first =
            TestPlayerBuilder::new(Arc::clone(&world), "First", 3)
                .uuid(Uuid::from_u128(3))
                .build();
        let second =
            TestPlayerBuilder::new(Arc::clone(&world), "Second", 4)
                .uuid(Uuid::from_u128(4))
                .build();
        assert!(world.add_player(Arc::clone(&first), ResetReason::InitialJoin));
        assert!(world.add_player(Arc::clone(&second), ResetReason::InitialJoin));

        let red_x = MapDecorationEntry::new(
            RegistryReference::new(&vanilla_map_decoration_types::RED_X),
            32.0,
            -16.0,
            180.0,
        );
        let mansion = MapDecorationEntry::new(
            RegistryReference::new(&vanilla_map_decoration_types::MANSION),
            -24.0,
            16.0,
            180.0,
        );
        let static_decorations = MapDecorations::EMPTY
            .with_decoration("z-first".to_owned(), red_x)
            .with_decoration("a-second".to_owned(), mansion);
        for player in [&first, &second] {
            let mut map = ItemStack::new(&vanilla_items::FILLED_MAP);
            map.set(MAP_ID, map_id);
            map.set(MAP_DECORATIONS, static_decorations.clone());
            player.inventory.lock().items_mut()[0] = map;
        }

        let first_carried = first
            .inventory
            .lock()
            .items()
            .iter()
            .filter_map(CarriedMap::from_item)
            .collect::<Vec<_>>();
        assert_eq!(
            store.synchronize_player(&first, &first_carried, &[]).len(),
            1
        );

        let second_carried = second
            .inventory
            .lock()
            .items()
            .iter()
            .filter_map(CarriedMap::from_item)
            .collect::<Vec<_>>();
        let packets = store.synchronize_player(&second, &second_carried, &[]);
        let Some(decorations) = &packets[0].decorations else {
            panic!("first update for the second holder should contain decorations");
        };
        let types = decorations
            .iter()
            .map(|decoration| decoration.decoration_type.value().key.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            types,
            [
                vanilla_map_decoration_types::PLAYER.key.clone(),
                vanilla_map_decoration_types::RED_X.key.clone(),
                vanilla_map_decoration_types::MANSION.key.clone(),
                vanilla_map_decoration_types::PLAYER.key.clone(),
            ]
        );
    }

    #[test]
    fn nether_rotation_uses_domain_world_identity_not_dimension_type() {
        let mut map = MapItemSavedData {
            center_x: 0,
            center_z: 0,
            world: Identifier::new("domain", "custom_nether"),
            dimension_type: vanilla_dimension_types::THE_NETHER.key.clone(),
            tracking_position: true,
            unlimited_tracking: true,
            scale: 0,
            colors: vec![0; MAP_COLOR_COUNT],
            locked: false,
        };

        assert_eq!(map_rotation(&map, 90.0, 20), 4);
        map.world = Identifier::new("domain", NETHER_WORLD_NAME);
        map.dimension_type = vanilla_dimension_types::OVERWORLD.key.clone();
        assert_eq!(map_rotation(&map, 90.0, 20), 13);
    }
}
