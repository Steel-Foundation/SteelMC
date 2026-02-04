use std::sync::Arc;

use steel_protocol::packet_traits::{ClientPacket, EncodedPacket};
use steel_protocol::packets::common::TagCollection;
use steel_protocol::{
    packets::{
        common::CUpdateTags,
        config::{CRegistryData, RegistryEntry},
    },
    utils::ConnectionProtocol,
};

use steel_registry::{
    BANNER_PATTERN_REGISTRY, BIOMES_REGISTRY, BLOCKS_REGISTRY, CAT_VARIANT_REGISTRY,
    CHAT_TYPE_REGISTRY, CHICKEN_VARIANT_REGISTRY, COW_VARIANT_REGISTRY, DAMAGE_TYPE_REGISTRY,
    DIALOG_REGISTRY, DIMENSION_TYPE_REGISTRY, FLUID_REGISTRY, FROG_VARIANT_REGISTRY,
    INSTRUMENT_REGISTRY, ITEMS_REGISTRY, JUKEBOX_SONG_REGISTRY, PAINTING_VARIANT_REGISTRY,
    PIG_VARIANT_REGISTRY, REGISTRY, Registry, TIMELINE_REGISTRY, TRIM_MATERIAL_REGISTRY,
    TRIM_PATTERN_REGISTRY, WOLF_SOUND_VARIANT_REGISTRY, WOLF_VARIANT_REGISTRY,
    ZOMBIE_NAUTILUS_VARIANT_REGISTRY,
};
use steel_utils::Identifier;
use steel_utils::codec::VarInt;

use crate::config::STEEL_CONFIG;

/// Caches compressed registry packets to avoid re-compressing them for every player.
pub struct RegistryCache {
    /// The cached registry data packets.
    pub registry_packets: Arc<[EncodedPacket]>,
    /// The cached tags packet.
    pub tags_packet: Arc<EncodedPacket>,
}

impl Default for RegistryCache {
    fn default() -> Self {
        Self::new()
    }
}

impl RegistryCache {
    /// Creates a new `RegistryCache` from the given registry.
    #[must_use]
    pub fn new() -> Self {
        let registry_packets = Self::build_registry_packets(&REGISTRY);
        let tags_by_registry_packet = Self::build_tags_packet(&REGISTRY);

        let (registry_packets, tags_packet) =
            build_compressed_packets(registry_packets, tags_by_registry_packet);

        Self {
            registry_packets,
            tags_packet: Arc::new(tags_packet),
        }
    }

    fn build_registry_packets(registry: &Registry) -> Vec<CRegistryData> {
        let mut packets = Vec::new();

        macro_rules! add_registry {
            ($reg_key:expr, $field:ident) => {
                packets.push(CRegistryData::new(
                    $reg_key,
                    registry
                        .$field
                        .iter()
                        .map(|(_, entry)| RegistryEntry::new(entry.key.clone(), None))
                        .collect(),
                ));
            };
        }

        //TODO: For non vanilla entries we need to encode the data into nbt

        add_registry!(BIOMES_REGISTRY, biomes);
        add_registry!(CHAT_TYPE_REGISTRY, chat_types);
        add_registry!(TRIM_PATTERN_REGISTRY, trim_patterns);
        add_registry!(TRIM_MATERIAL_REGISTRY, trim_materials);
        add_registry!(WOLF_VARIANT_REGISTRY, wolf_variants);
        add_registry!(WOLF_SOUND_VARIANT_REGISTRY, wolf_sound_variants);
        add_registry!(PIG_VARIANT_REGISTRY, pig_variants);
        add_registry!(FROG_VARIANT_REGISTRY, frog_variants);
        add_registry!(CAT_VARIANT_REGISTRY, cat_variants);
        add_registry!(COW_VARIANT_REGISTRY, cow_variants);
        add_registry!(CHICKEN_VARIANT_REGISTRY, chicken_variants);
        add_registry!(PAINTING_VARIANT_REGISTRY, painting_variants);
        add_registry!(DIMENSION_TYPE_REGISTRY, dimension_types);
        add_registry!(DAMAGE_TYPE_REGISTRY, damage_types);
        add_registry!(BANNER_PATTERN_REGISTRY, banner_patterns);
        add_registry!(ZOMBIE_NAUTILUS_VARIANT_REGISTRY, zombie_nautilus_variants);

        // TODO: Add enchantments when implemented in the registry
        //add_registry!(Identifier::vanilla_static("enchantments"), enchantments);

        add_registry!(JUKEBOX_SONG_REGISTRY, jukebox_songs);
        add_registry!(INSTRUMENT_REGISTRY, instruments);
        add_registry!(TIMELINE_REGISTRY, timelines);
        add_registry!(DIALOG_REGISTRY, dialogs);

        packets
    }

    fn build_tags_packet(registry: &Registry) -> CUpdateTags {
        let mut tags_by_registry: TagCollection = Vec::with_capacity(2);

        // Build block tags
        let mut block_tags: Vec<(Identifier, Vec<VarInt>)> =
            Vec::with_capacity(registry.blocks.tag_keys().count());
        for tag_key in registry.blocks.tag_keys() {
            let mut block_ids = Vec::with_capacity(registry.blocks.iter_tag(tag_key).count());

            for block in registry.blocks.iter_tag(tag_key) {
                let block_id = *registry.blocks.get_id(block);
                block_ids.push(VarInt::from(block_id));
            }

            block_tags.push((tag_key.clone(), block_ids));
        }

        tags_by_registry.push((BLOCKS_REGISTRY, block_tags));

        // Build item tags
        let mut item_tags: Vec<(Identifier, Vec<VarInt>)> =
            Vec::with_capacity(registry.items.tag_keys().count());
        for tag_key in registry.items.tag_keys() {
            let mut item_ids = Vec::with_capacity(registry.items.iter_tag(tag_key).count());

            for item in registry.items.iter_tag(tag_key) {
                let item_id = *registry.items.get_id(item);
                item_ids.push(VarInt::from(item_id));
            }

            item_tags.push((tag_key.clone(), item_ids));
        }

        tags_by_registry.push((ITEMS_REGISTRY, item_tags));

        // Build timeline tags
        let mut timeline_tags: Vec<(Identifier, Vec<VarInt>)> =
            Vec::with_capacity(registry.timelines.tag_keys().count());
        for tag_key in registry.timelines.tag_keys() {
            let mut timeline_ids = Vec::with_capacity(registry.timelines.iter_tag(tag_key).count());

            for timeline in registry.timelines.iter_tag(tag_key) {
                let timeline_id = *registry.timelines.get_id(timeline);
                timeline_ids.push(VarInt::from(timeline_id));
            }

            timeline_tags.push((tag_key.clone(), timeline_ids));
        }

        tags_by_registry.push((TIMELINE_REGISTRY, timeline_tags));

        // Build dialog tags
        let mut dialog_tags: Vec<(Identifier, Vec<VarInt>)> =
            Vec::with_capacity(registry.dialogs.tag_keys().count());
        for tag_key in registry.dialogs.tag_keys() {
            let mut dialog_ids = Vec::with_capacity(registry.dialogs.iter_tag(tag_key).count());

            for dialog in registry.dialogs.iter_tag(tag_key) {
                let dialog_id = *registry.dialogs.get_id(dialog);
                dialog_ids.push(VarInt::from(dialog_id as i32));
            }

            dialog_tags.push((tag_key.clone(), dialog_ids));
        }

        tags_by_registry.push((DIALOG_REGISTRY, dialog_tags));

        // Build fluid tags
        let mut fluid_tags: Vec<(Identifier, Vec<VarInt>)> =
            Vec::with_capacity(registry.fluids.tag_keys().count());
        for tag_key in registry.fluids.tag_keys() {
            let mut fluid_ids = Vec::with_capacity(registry.fluids.iter_tag(tag_key).count());

            for fluid in registry.fluids.iter_tag(tag_key) {
                let fluid_id = *registry.fluids.get_id(fluid).expect("Fluid not found");
                fluid_ids.push(VarInt::from(fluid_id as i32));
            }

            fluid_tags.push((tag_key.clone(), fluid_ids));
        }
        tags_by_registry.push((FLUID_REGISTRY, fluid_tags));

        // Build and return a CUpdateTagsPacket based on the registry data
        CUpdateTags::new(tags_by_registry)
    }
}

/// Compresses a packet.
fn compress_packet<P: ClientPacket>(packet: P) -> Option<EncodedPacket> {
    let compression_info = STEEL_CONFIG.compression;
    let id = packet.get_id(ConnectionProtocol::Config);

    EncodedPacket::from_bare(packet, compression_info, ConnectionProtocol::Config)
        .map_err(|_| {
            log::error!("Failed to encode packet: {id:?}");
        })
        .ok()
}

/// # Panics
/// This function will panic if the compression fails.
#[must_use]
pub fn build_compressed_packets(
    registry_packets: Vec<CRegistryData>,
    tags_packet: CUpdateTags,
) -> (Arc<[EncodedPacket]>, EncodedPacket) {
    let mut compressed_packets = Vec::with_capacity(registry_packets.len());

    for packet in registry_packets {
        compressed_packets.push(compress_packet(packet).expect("Failed to compress packet"));
    }

    let compressed_tags_packet =
        compress_packet(tags_packet).expect("Failed to compress tags packet");

    (compressed_packets.into(), compressed_tags_packet)
}
