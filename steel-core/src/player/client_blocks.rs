//! Per-connection visual attachments for projected server blocks.
//!
//! These are client-only displays, not world entities. Their lifetime is the
//! client's chunk membership, owned under the same lock as chunk packet commits.

use glam::{DVec3, Vec3};
use rustc_hash::FxHashMap;
use std::mem;
use steel_protocol::packets::game::{CAddEntity, CRemoveEntities, CSetEntityData};
use steel_protocol::{packet_traits::EncodedPacket, utils::ConnectionProtocol};
use steel_registry::{
    REGISTRY, RegistryEntry,
    blocks::block_state_ext::BlockStateExt,
    entity_data::{Quaternionf, Vector3f},
    vanilla_entities,
    vanilla_entity_data::ItemDisplayEntityData,
};
use steel_utils::{BlockPos, BlockStateId, ChunkPos};
use uuid::Uuid;

use super::{
    PlayerConnection,
    connection::{BundleBuilder, NetworkConnection},
};
use crate::{chunk::full_chunk::FullChunkRef, entity::next_entity_id};

#[derive(Debug)]
struct DisplayGroup {
    state: BlockStateId,
    ids: Vec<i32>,
}

#[derive(Debug, Default)]
pub(crate) struct ClientBlockDisplays {
    chunks: FxHashMap<ChunkPos, FxHashMap<BlockPos, DisplayGroup>>,
}

impl ClientBlockDisplays {
    /// The snapshot is collected in the same revision window as the chunk packet.
    pub(crate) fn snapshot(chunk: FullChunkRef<'_>) -> Vec<(BlockPos, BlockStateId)> {
        if !REGISTRY.client_content.has_blocks() {
            return Vec::new();
        }
        let mut blocks = Vec::new();
        let chunk_pos = chunk.common().pos();
        for (section_index, holder) in chunk.sections().sections.iter().enumerate() {
            let section = holder.read();
            let states = section.states();
            if !states.maybe_has(|state| REGISTRY.client_content.block(state.get_block()).is_some())
            {
                continue;
            }
            for index in 0..16 * 16 * 16 {
                let state = states.get_at_index(index);
                if REGISTRY.client_content.block(state.get_block()).is_none() {
                    continue;
                }
                blocks.push((
                    BlockPos::new(
                        chunk_pos.0.x * 16 + (index & 15) as i32,
                        chunk.min_y() + section_index as i32 * 16 + (index >> 8) as i32,
                        chunk_pos.0.y * 16 + ((index >> 4) & 15) as i32,
                    ),
                    state,
                ));
            }
        }
        blocks
    }

    pub(crate) fn replace_chunk(
        &mut self,
        connection: &PlayerConnection,
        chunk: ChunkPos,
        blocks: &[(BlockPos, BlockStateId)],
    ) {
        self.remove_chunk(connection, chunk);
        for &(pos, state) in blocks {
            self.update(connection, pos, state);
        }
    }

    pub(crate) fn update(
        &mut self,
        connection: &PlayerConnection,
        pos: BlockPos,
        state: BlockStateId,
    ) {
        if connection.closed() {
            return;
        }
        let chunk_pos = ChunkPos::from_block_pos(pos);
        let block = state.get_block();
        if let Some(group) = self
            .chunks
            .get(&chunk_pos)
            .and_then(|blocks| blocks.get(&pos))
            && group.state == state
        {
            return;
        }
        let old = self
            .chunks
            .get_mut(&chunk_pos)
            .and_then(|blocks| blocks.remove(&pos));
        let model = REGISTRY.client_content.block(block);
        let Some(model) = model.filter(|model| !model.displays_for(state).is_empty()) else {
            if let Some(old) = old {
                Self::send_removals(connection, old.ids);
            }
            if self.chunks.get(&chunk_pos).is_some_and(FxHashMap::is_empty) {
                self.chunks.remove(&chunk_pos);
            }
            return;
        };

        let displays = model.displays_for(state);
        let mut ids = Vec::with_capacity(displays.len());
        let mut bundle = BundleBuilder::new(connection.compression());
        if let Some(old) = old {
            bundle.add(CRemoveEntities {
                entity_ids: old.ids,
            });
        }
        for display in displays {
            let id = next_entity_id();
            let mut data = ItemDisplayEntityData::new();
            data.item_stack.set(display.item.clone());
            // Vanilla probes display light at its entity position. Keep that
            // probe above the opaque carrier and compensate the model origin.
            let anchor = Vec3::new(0.5, 1.0, 0.5);
            let translation = display.translation - anchor;
            data.display.translation.set(Vector3f::new(
                translation.x,
                translation.y,
                translation.z,
            ));
            data.display.scale.set(Vector3f::new(
                display.scale.x,
                display.scale.y,
                display.scale.z,
            ));
            let rotation = display.rotation;
            data.display.left_rotation.set(Quaternionf::new(
                rotation.x, rotation.y, rotation.z, rotation.w,
            ));
            data.display.base.no_gravity.set(true);
            // Display passenger/vehicle attachments are zero in vanilla, so
            // the supplied composition needs no invisible parent entity.
            bundle.add(CAddEntity {
                id,
                uuid: Uuid::new_v4(),
                entity_type: vanilla_entities::ITEM_DISPLAY.id() as i32,
                position: DVec3::new(f64::from(pos.x()), f64::from(pos.y()), f64::from(pos.z()))
                    + anchor.as_dvec3(),
                velocity: DVec3::ZERO,
                x_rot: 0,
                y_rot: 0,
                head_y_rot: 0,
                data: 0,
            });
            bundle.add(CSetEntityData::new(id, data.pack_all()));
            ids.push(id);
        }
        connection.send_encoded_bundle(bundle.into_packets());
        self.chunks
            .entry(chunk_pos)
            .or_default()
            .insert(pos, DisplayGroup { state, ids });
    }

    pub(crate) fn remove_chunk(&mut self, connection: &PlayerConnection, chunk: ChunkPos) {
        if let Some(blocks) = self.chunks.remove(&chunk)
            && !connection.closed()
        {
            let entity_ids = blocks.into_values().flat_map(|group| group.ids).collect();
            Self::send_removals(connection, entity_ids);
        }
    }

    pub(crate) fn clear(&mut self, connection: &PlayerConnection) {
        let chunks = mem::take(&mut self.chunks);
        if connection.closed() || chunks.is_empty() {
            return;
        }
        let entity_ids = chunks
            .into_values()
            .flat_map(FxHashMap::into_values)
            .flat_map(|group| group.ids)
            .collect();
        Self::send_removals(connection, entity_ids);
    }

    fn send_removals(connection: &PlayerConnection, entity_ids: Vec<i32>) {
        match EncodedPacket::from_bare(
            CRemoveEntities { entity_ids },
            connection.compression(),
            ConnectionProtocol::Play,
        ) {
            Ok(packet) => connection.send_encoded(packet),
            Err(error) => log::warn!("failed to encode client block display removal: {error}"),
        }
    }
}
