use steel_registry::blocks::BlockRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks::{
    BARRIER, CARVED_PUMPKIN, JACK_O_LANTERN, MANGROVE_LEAVES, MELON, PUMPKIN,
};

pub fn is_excluded_for_connection(block: BlockRef) -> bool {
    block.has_tag(&BlockTag::LEAVES)
        || block == &BARRIER
        || block == &CARVED_PUMPKIN
        || block == &JACK_O_LANTERN
        || block == &MELON
        || block == &PUMPKIN
        || block.has_tag(&BlockTag::SHULKER_BOXES)
        || block == &MANGROVE_LEAVES
}
