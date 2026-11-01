//! Real native mining outcomes and trap placement.
use super::fixture;
use glam::DVec3;
use std::error::Error;
use steel_core::{
    entity::{Entity as _, entities::objects::items::ItemEntity},
    player::game_mode::block_breaking::{BlockBreakAction, BlockBreakingManager},
};
use steel_registry::{
    blocks::{block_state_ext::BlockStateExt, properties::Direction},
    item_stack::ItemStack,
    items::ItemRef,
    vanilla_blocks, vanilla_items,
};
use steel_utils::{
    BlockPos, BlockStateId, Downcast as _, WorldAabb,
    random::{Random, xoroshiro::Xoroshiro},
    types::{GameType, UpdateFlags},
};

fn mine(fixture: &fixture::Fixture, placed: BlockPos, state: BlockStateId) {
    let world = &fixture.world;
    let player = &fixture.player;
    // Hand mining follows the actual survival path, with the carrier's hardness.
    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::empty());
    let mut breaking = BlockBreakingManager::new();
    breaking.handle_block_break_action(
        player,
        world,
        placed,
        BlockBreakAction::Start,
        Direction::West,
    );
    assert_eq!(
        world.get_block_state(placed),
        state,
        "oak planks must not break instantly by hand"
    );
    for _ in 0..80 {
        breaking.tick(player, world);
    }
    breaking.handle_block_break_action(
        player,
        world,
        placed,
        BlockBreakAction::Stop,
        Direction::West,
    );
    assert!(world.get_block_state(placed).is_air());
}

fn item_drops(fixture: &fixture::Fixture) -> Vec<ItemStack> {
    let drops = fixture.world.get_entities_in_aabb(&WorldAabb::of_size(
        DVec3::new(8.5, 65.5, 8.5),
        16.0,
        16.0,
        16.0,
    ));
    drops
        .iter()
        .filter_map(|entity| entity.downcast_ref::<ItemEntity>())
        .map(ItemEntity::get_item)
        .collect()
}

pub(super) fn check(
    fixture: &fixture::Fixture,
    item: ItemRef,
    placed: BlockPos,
    state: BlockStateId,
) -> Result<(), Box<dyn Error>> {
    let world = &fixture.world;
    check_creative(fixture, placed);
    let mut random = Xoroshiro::from_seed(42);
    let rewards = [
        (&*vanilla_items::DIAMOND, 3),
        (&*vanilla_items::EMERALD, 5),
        (&*vanilla_items::IRON_INGOT, 8),
        (&*vanilla_items::GOLD_INGOT, 8),
        (&*vanilla_items::BREAD, 8),
        (&*vanilla_items::COAL, 16),
    ];
    let sentinel = BlockPos::new(5, 65, 7);
    assert!(world.set_block(
        sentinel,
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_ALL
    ));
    let mut seen = [false; 8];
    for _ in 0..64 {
        // Clear only the previous trap's footprint; keep the existing solid block.
        for y in 65..=67 {
            for z in 7..=9 {
                for x in 5..=7 {
                    let pos = BlockPos::new(x, y, z);
                    if pos != sentinel {
                        world.set_block(
                            pos,
                            vanilla_blocks::AIR.default_state(),
                            UpdateFlags::UPDATE_ALL,
                        );
                    }
                }
            }
        }
        world.set_block(placed, state, UpdateFlags::UPDATE_ALL);
        let before = item_drops(fixture);
        let outcome = random.next_i32_bounded(8) as usize;
        seen[outcome] = true;
        mine(fixture, placed, state);
        let after = item_drops(fixture);
        assert!(
            !after.iter().any(|stack| stack.is(item)),
            "opening must not return the lucky block"
        );
        if let Some(&(reward, count)) = rewards.get(outcome) {
            let count_of = |stacks: &[ItemStack]| {
                stacks
                    .iter()
                    .filter(|stack| stack.is(reward))
                    .map(ItemStack::count)
                    .sum::<i32>()
            };
            assert_eq!(after.len(), before.len() + 1);
            assert_eq!(count_of(&after) - count_of(&before), count);
            assert!(world.get_block_state(BlockPos::new(6, 65, 8)).is_air());
        } else {
            assert_eq!(after.len(), before.len());
            if outcome == 6 {
                assert_eq!(
                    world.get_block_state(BlockPos::new(6, 65, 8)),
                    vanilla_blocks::COBWEB.default_state()
                );
                assert_eq!(
                    world.get_block_state(BlockPos::new(7, 66, 9)),
                    vanilla_blocks::COBWEB.default_state()
                );
            } else {
                assert!(
                    world.get_block_state(BlockPos::new(6, 65, 8)).is_air(),
                    "the cage must leave the miner's body space clear"
                );
                assert_eq!(
                    world.get_block_state(BlockPos::new(7, 65, 8)),
                    vanilla_blocks::OBSIDIAN.default_state()
                );
                assert_eq!(
                    world.get_block_state(BlockPos::new(6, 67, 8)),
                    vanilla_blocks::OBSIDIAN.default_state()
                );
            }
        }
        assert_eq!(
            world.get_block_state(sentinel),
            vanilla_blocks::STONE.default_state()
        );
    }
    assert!(
        seen.into_iter().all(|seen| seen),
        "the seeded run must exercise every reward and both traps"
    );

    check_boundary(fixture, state, &mut random)?;
    Ok(())
}

fn check_creative(fixture: &fixture::Fixture, placed: BlockPos) {
    let world = &fixture.world;
    assert!(fixture.player.set_game_mode(GameType::Creative));
    let mut creative = BlockBreakingManager::new();
    creative.handle_block_break_action(
        &fixture.player,
        world,
        placed,
        BlockBreakAction::Start,
        Direction::West,
    );
    assert!(world.get_block_state(placed).is_air());
    assert_eq!(item_drops(fixture), []);
    for y in 65..=67 {
        for z in 7..=9 {
            for x in 5..=7 {
                assert!(world.get_block_state(BlockPos::new(x, y, z)).is_air());
            }
        }
    }
    assert!(fixture.player.set_game_mode(GameType::Survival));
}

fn check_boundary(
    fixture: &fixture::Fixture,
    state: BlockStateId,
    random: &mut Xoroshiro,
) -> Result<(), Box<dyn Error>> {
    let world = &fixture.world;
    // A footprint crossing an unloaded chunk must not apply half of a trap.
    let boundary = BlockPos::new(13, 65, 14);
    fixture
        .player
        .try_set_position(DVec3::new(15.5, 65.0, 15.5))?;
    let mut checked_boundary_trap = false;
    for _ in 0..32 {
        world.set_block(boundary, state, UpdateFlags::UPDATE_ALL);
        let outcome = random.next_i32_bounded(8);
        let before = item_drops(fixture).len();
        mine(fixture, boundary, state);
        if outcome < 6 {
            continue;
        }
        assert_eq!(item_drops(fixture).len(), before);
        for y in 65..=67 {
            for z in 14..=15 {
                for x in 14..=15 {
                    assert!(world.get_block_state(BlockPos::new(x, y, z)).is_air());
                }
            }
        }
        checked_boundary_trap = true;
        break;
    }
    assert!(
        checked_boundary_trap,
        "the seeded run must reach a trap at the boundary"
    );
    // Ordinary carrier blocks keep their original drops and never trigger an outcome.
    world.set_block(
        boundary,
        vanilla_blocks::OAK_PLANKS.default_state(),
        UpdateFlags::UPDATE_ALL,
    );
    let before = item_drops(fixture).len();
    mine(
        fixture,
        boundary,
        vanilla_blocks::OAK_PLANKS.default_state(),
    );
    let after = item_drops(fixture);
    assert_eq!(after.len(), before + 1);
    assert!(
        after
            .iter()
            .any(|stack| stack.is(&vanilla_items::OAK_PLANKS) && stack.count() == 1)
    );
    Ok(())
}
