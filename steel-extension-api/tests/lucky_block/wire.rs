use std::{error::Error, io::Cursor};

use steel_core::chunk::paletted_container::BlockPalette;
use steel_protocol::packets::game::{CAwardStats, CBlockUpdate, CLevelEvent};
use steel_registry::{
    REGISTRY,
    blocks::BlockRef,
    data_components::components::{ToolRule, ToolRuleBlocks},
    items::ItemRef,
    level_events,
    stat::{Stat, vanilla_stat_types},
    vanilla_blocks, vanilla_items,
};
use steel_utils::{
    BlockPos, BlockStateId,
    codec::VarInt,
    mth::ceil_log2,
    serial::{ReadFrom, WriteTo},
};

pub fn check(block: BlockRef, item: ItemRef) -> Result<(), Box<dyn Error>> {
    let custom = block.default_state();
    let carrier = vanilla_blocks::OAK_PLANKS.default_state();
    let custom_rule = ToolRule::override_speed(ToolRuleBlocks::Direct(vec![block]), 7.0);
    assert!(custom_rule.matches_block(custom));
    assert!(!custom_rule.matches_block(carrier));
    let mut actual = Vec::new();
    let mut expected = Vec::new();
    CBlockUpdate {
        pos: BlockPos::ZERO,
        block_state: custom,
    }
    .write(&mut actual)?;
    CBlockUpdate {
        pos: BlockPos::ZERO,
        block_state: carrier,
    }
    .write(&mut expected)?;
    assert_eq!(actual, expected);
    for event_type in [
        level_events::PARTICLES_DESTROY_BLOCK,
        level_events::PARTICLES_AND_SOUND_DESTROY_BLOCK,
        level_events::PARTICLES_AND_SOUND_BRUSH_BLOCK_COMPLETE,
    ] {
        actual.clear();
        expected.clear();
        CLevelEvent::new(event_type, BlockPos::ZERO, i32::from(custom.0), false)
            .write(&mut actual)?;
        CLevelEvent::new(event_type, BlockPos::ZERO, i32::from(carrier.0), false)
            .write(&mut expected)?;
        assert_eq!(actual, expected);
    }

    // The server counts custom-item statistics, but the vanilla client must not
    // receive their server-only IDs or have its head statistic overwritten.
    let custom_stat = vanilla_stat_types::ITEM_USED.get(item);
    let vanilla_stat = vanilla_stat_types::ITEM_USED.get(&vanilla_items::PLAYER_HEAD);
    actual.clear();
    CAwardStats {
        stats: vec![(custom_stat, 3), (vanilla_stat, 5)],
    }
    .write(&mut actual)?;
    let mut data = Cursor::new(actual.as_slice());
    assert_eq!(VarInt::read(&mut data)?.0, 1);
    assert_eq!(Stat::read(&mut data)?, vanilla_stat);
    assert_eq!(VarInt::read(&mut data)?.0, 5);
    assert_eq!(custom_stat.stat_value_key(), &item.key);
    check_global_palette(custom, carrier)
}

fn check_global_palette(custom: BlockStateId, carrier: BlockStateId) -> Result<(), Box<dyn Error>> {
    let mut palette = BlockPalette::Homogeneous(vanilla_blocks::AIR.default_state());
    // Enough distinct registered blocks to exercise the global palette path.
    for (index, (_, block)) in REGISTRY.blocks.iter().take(257).enumerate() {
        palette.set(
            index & 15,
            index >> 8,
            (index >> 4) & 15,
            block.default_state(),
        );
    }
    let index = 257;
    palette.set(index & 15, index >> 8, (index >> 4) & 15, custom);
    let mut bytes = Vec::new();
    palette.write(&mut bytes)?;
    let mut data = Cursor::new(bytes.as_slice());
    let bits = u8::read(&mut data)?;
    assert_eq!(
        bits,
        ceil_log2(REGISTRY.client_content.vanilla_state_count())
    );
    let per_word = 64 / usize::from(bits);
    let mask = (1_u64 << bits) - 1;
    let mut words = Vec::new();
    while (data.position() as usize) < bytes.len() {
        let word = u64::read(&mut data)?;
        for position in 0..per_word {
            assert!(
                ((word >> (position * usize::from(bits))) & mask)
                    < REGISTRY.client_content.vanilla_state_count() as u64
            );
        }
        words.push(word);
    }
    let wire_id = (words[index / per_word] >> ((index % per_word) * usize::from(bits))) & mask;
    assert_eq!(wire_id, u64::from(carrier.0));
    Ok(())
}
