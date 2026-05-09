use steel_registry::blocks::BlockRef;
use steel_registry::vanilla_block_tags::{LEAVES_TAG, SHULKER_BOXES_TAG};
use steel_registry::vanilla_blocks::{
    BARRIER, CARVED_PUMPKIN, JACK_O_LANTERN, MANGROVE_LEAVES, MELON, PUMPKIN,
};
use steel_registry::{REGISTRY, TaggedRegistryExt};

pub fn is_excluded_for_connection(block: BlockRef) -> bool {
    REGISTRY.blocks.is_in_tag(block, &LEAVES_TAG)
        || block == &BARRIER
        || block == &CARVED_PUMPKIN
        || block == &JACK_O_LANTERN
        || block == &MELON
        || block == &PUMPKIN
        || REGISTRY.blocks.is_in_tag(block, &SHULKER_BOXES_TAG)
        || block == &MANGROVE_LEAVES
}
