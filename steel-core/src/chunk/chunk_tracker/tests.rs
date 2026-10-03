use super::*;
use crate::chunk::simulation_ticket_manager::SimulationTicketManager;

const DETERMINISTIC_SEQUENCE_MULTIPLIER: u64 = 6_364_136_223_846_793_005;
const DETERMINISTIC_SEQUENCE_INCREMENT: u64 = 1_442_695_040_888_963_407;

struct DeterministicSequence(u64);

impl DeterministicSequence {
    fn next(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(DETERMINISTIC_SEQUENCE_MULTIPLIER)
            .wrapping_add(DETERMINISTIC_SEQUENCE_INCREMENT);
        let bound = u64::try_from(bound).expect("test bound must fit in u64");
        let value = (self.0 >> u32::BITS) % bound;
        usize::try_from(value).expect("bounded test value must fit in usize")
    }
}

const fn source(pos: ChunkPos, level: Option<ChunkTicketLevel>) -> SourceLevelUpdate {
    SourceLevelUpdate { pos, level }
}

const fn entity_ticking_source(pos: ChunkPos, radius: u8) -> SourceLevelUpdate {
    source(
        pos,
        Some(ChunkTicketLevel::for_entity_ticking_radius(radius)),
    )
}

fn has_change(
    changes: &[ChunkLevelChange],
    pos: ChunkPos,
    new_level: Option<ChunkTicketLevel>,
) -> bool {
    changes.contains(&ChunkLevelChange { pos, new_level })
}

fn add_reference_source<const MAX_LEVEL: u8>(
    levels: &mut FxHashMap<ChunkPos, ChunkTicketLevel>,
    source_pos: ChunkPos,
    source_level: u8,
) {
    if source_level > MAX_LEVEL {
        return;
    }

    let radius = i32::from(MAX_LEVEL - source_level);
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            let distance = u8::try_from(dx.unsigned_abs().max(dz.unsigned_abs()))
                .expect("reference distance must fit in u8");
            let raw_level = source_level
                .checked_add(distance)
                .expect("reference level must fit in u8");
            let Some(level) = ChunkTicketLevel::new(raw_level) else {
                panic!("reference produced an invalid simulation level");
            };
            let pos = ChunkPos::new(source_pos.0.x + dx, source_pos.0.y + dz);
            levels
                .entry(pos)
                .and_modify(|stored| *stored = (*stored).min(level))
                .or_insert(level);
        }
    }
}

fn reference_levels<const MAX_LEVEL: u8>(
    manager: &ChunkTracker<MAX_LEVEL>,
) -> FxHashMap<ChunkPos, ChunkTicketLevel> {
    let mut levels = FxHashMap::default();
    for (&pos, &source_level) in &manager.source_levels {
        add_reference_source::<MAX_LEVEL>(&mut levels, pos, source_level);
    }
    levels
}

fn reference_changes(
    old_levels: &FxHashMap<ChunkPos, ChunkTicketLevel>,
    new_levels: &FxHashMap<ChunkPos, ChunkTicketLevel>,
) -> Vec<ChunkLevelChange> {
    let mut changes = Vec::new();
    for (&pos, &new_level) in new_levels {
        if old_levels.get(&pos) != Some(&new_level) {
            changes.push(ChunkLevelChange {
                pos,
                new_level: Some(new_level),
            });
        }
    }
    for &pos in old_levels.keys() {
        if !new_levels.contains_key(&pos) {
            changes.push(ChunkLevelChange {
                pos,
                new_level: None,
            });
        }
    }
    changes.sort_unstable_by_key(|change| (change.pos.0.x, change.pos.0.y));
    changes
}

fn run_and_compare_with_reference<const MAX_LEVEL: u8>(
    manager: &mut ChunkTracker<MAX_LEVEL>,
    previous_levels: &mut FxHashMap<ChunkPos, ChunkTicketLevel>,
) {
    let actual_changes = manager.run_all_updates().to_vec();
    let expected_levels = reference_levels(manager);
    let expected_changes = reference_changes(previous_levels, &expected_levels);

    assert_eq!(manager.levels, expected_levels);
    assert_eq!(actual_changes, expected_changes);
    *previous_levels = expected_levels;
}

#[test]
fn overlapping_sources_keep_the_strongest_propagated_level() {
    let mut manager = SimulationTicketManager::new();
    manager.apply_source_updates([
        entity_ticking_source(ChunkPos::new(0, 0), 2),
        entity_ticking_source(ChunkPos::new(4, 0), 0),
    ]);
    manager.run_all_updates();

    assert_eq!(
        manager.get_level(ChunkPos::new(2, 0)),
        Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK)
    );
    assert_eq!(
        manager.get_level(ChunkPos::new(3, 0)),
        Some(ChunkTicketLevel::BLOCK_TICKING_CHUNK)
    );
    assert_eq!(
        manager.get_level(ChunkPos::new(4, 0)),
        Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK)
    );
}

#[test]
fn repeated_updates_coalesce_to_the_original_level() {
    let mut manager = SimulationTicketManager::new();
    let pos = ChunkPos::new(0, 0);
    manager.apply_source_update(entity_ticking_source(pos, 2));
    manager.apply_source_update(entity_ticking_source(pos, 0));
    manager.apply_source_update(source(pos, None));

    assert_eq!(manager.run_all_updates(), []);
    assert_eq!(manager.run_all_updates(), []);
    assert_eq!(manager.get_level(pos), None);

    manager.apply_source_updates([source(pos, None), entity_ticking_source(pos, 2)]);
    assert_ne!(manager.run_all_updates(), []);
    manager.apply_source_update(entity_ticking_source(pos, 0));
    manager.apply_source_update(entity_ticking_source(pos, 2));

    assert_eq!(manager.run_all_updates(), []);
    assert_eq!(
        manager.get_level(pos),
        Some(ChunkTicketLevel::for_entity_ticking_radius(2))
    );
}

#[test]
#[should_panic(expected = "source level exceeds tracker limit")]
fn load_only_level_cannot_alias_an_absent_simulation_source() {
    let mut manager = SimulationTicketManager::new();
    manager.apply_source_update(SourceLevelUpdate {
        pos: ChunkPos::new(0, 0),
        level: Some(ChunkTicketLevel::FULL_CHUNK),
    });
}

#[test]
fn weakening_a_source_removes_its_old_outer_levels() {
    let mut manager = SimulationTicketManager::new();
    let pos = ChunkPos::new(0, 0);
    manager.apply_source_update(entity_ticking_source(pos, 2));
    manager.run_all_updates();

    manager.apply_source_update(entity_ticking_source(pos, 0));
    let changes = manager.run_all_updates();

    assert!(has_change(
        changes,
        pos,
        Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK)
    ));
    assert!(has_change(changes, ChunkPos::new(2, 0), None));
    assert_eq!(
        manager.get_level(pos),
        Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK)
    );
}

#[test]
fn source_batch_order_does_not_change_the_result() {
    let positions = [
        ChunkPos::new(0, 0),
        ChunkPos::new(6, 1),
        ChunkPos::new(-3, 2),
        ChunkPos::new(4, -2),
    ];
    let levels = [
        Some(ChunkTicketLevel::for_entity_ticking_radius(4)),
        Some(ChunkTicketLevel::for_entity_ticking_radius(1)),
        Some(ChunkTicketLevel::for_entity_ticking_radius(2)),
        Some(ChunkTicketLevel::for_entity_ticking_radius(0)),
    ];
    let mut forwards = SimulationTicketManager::new();
    let mut backwards = SimulationTicketManager::new();

    forwards.apply_source_updates(
        positions
            .into_iter()
            .zip(levels)
            .map(|(pos, level)| source(pos, level)),
    );
    backwards.apply_source_updates(
        positions
            .into_iter()
            .zip(levels)
            .rev()
            .map(|(pos, level)| source(pos, level)),
    );

    assert_eq!(forwards.run_all_updates(), backwards.run_all_updates());
    assert_eq!(forwards.levels, backwards.levels);
}

#[test]
fn incremental_source_updates_match_reference() {
    let mut manager = SimulationTicketManager::new();
    let mut previous_levels = FxHashMap::default();
    manager.apply_source_updates([
        entity_ticking_source(ChunkPos::new(0, 0), 4),
        entity_ticking_source(ChunkPos::new(6, 1), 1),
        entity_ticking_source(ChunkPos::new(-3, 2), 2),
        entity_ticking_source(ChunkPos::new(4, -2), 0),
    ]);
    run_and_compare_with_reference(&mut manager, &mut previous_levels);

    for level in [
        Some(ChunkTicketLevel::for_entity_ticking_radius(0)),
        Some(ChunkTicketLevel::for_entity_ticking_radius(1)),
        Some(ChunkTicketLevel::for_entity_ticking_radius(3)),
        Some(ChunkTicketLevel::BLOCK_TICKING_CHUNK),
    ] {
        manager.apply_source_update(source(ChunkPos::new(-3, 2), level));
        run_and_compare_with_reference(&mut manager, &mut previous_levels);
    }

    manager.apply_source_updates([
        source(ChunkPos::new(0, 0), None),
        source(ChunkPos::new(-3, 2), None),
        entity_ticking_source(ChunkPos::new(9, 3), 3),
        source(ChunkPos::new(4, -2), None),
        source(ChunkPos::new(6, 1), None),
    ]);
    run_and_compare_with_reference(&mut manager, &mut previous_levels);
}

#[test]
fn deterministic_random_operations_match_reference() {
    let mut manager = SimulationTicketManager::new();
    let mut previous_levels = FxHashMap::default();
    let mut sequence = DeterministicSequence(0x5eed_cafe_d00d_f00d);
    let source_levels = [
        None,
        Some(ChunkTicketLevel::for_entity_ticking_radius(4)),
        Some(ChunkTicketLevel::for_entity_ticking_radius(2)),
        Some(ChunkTicketLevel::for_entity_ticking_radius(0)),
        Some(ChunkTicketLevel::BLOCK_TICKING_CHUNK),
    ];

    for _ in 0..200 {
        for _ in 0..=sequence.next(3) {
            let x = i32::try_from(sequence.next(13)).expect("test x must fit in i32") - 6;
            let z = i32::try_from(sequence.next(13)).expect("test z must fit in i32") - 6;
            let pos = ChunkPos::new(x, z);
            let level = source_levels[sequence.next(source_levels.len())];
            manager.apply_source_update(source(pos, level));
        }

        run_and_compare_with_reference(&mut manager, &mut previous_levels);
    }
}

#[test]
fn loading_updates_match_reference_across_generation_levels() {
    use crate::chunk::chunk_ticket_manager::{LoadTicketManager, ticket_level_for_status};
    use crate::chunk::status::ChunkStatus;

    let mut manager = LoadTicketManager::new();
    let mut previous_levels = FxHashMap::default();
    let mut sequence = DeterministicSequence(0x0577_cafe);
    let source_levels = [
        None,
        Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK),
        Some(ChunkTicketLevel::FULL_CHUNK),
        Some(ticket_level_for_status(ChunkStatus::Biomes)),
        Some(ChunkTicketLevel::MAX),
    ];

    for _ in 0..200 {
        for _ in 0..=sequence.next(4) {
            let x = i32::try_from(sequence.next(25)).expect("bounded coordinate") - 12;
            let z = i32::try_from(sequence.next(25)).expect("bounded coordinate") - 12;
            manager.apply_source_update(source(
                ChunkPos::new(x, z),
                source_levels[sequence.next(source_levels.len())],
            ));
        }
        run_and_compare_with_reference(&mut manager, &mut previous_levels);
    }

    let positions: Vec<_> = manager.source_levels.keys().copied().collect();
    manager.apply_source_updates(positions.into_iter().map(|pos| source(pos, None)));
    run_and_compare_with_reference(&mut manager, &mut previous_levels);
    assert!(manager.levels.is_empty());
}

#[test]
fn moving_player_loading_coverage_preserves_overlaps_and_generation_moat() {
    use crate::chunk::chunk_ticket_manager::LoadTicketManager;
    use crate::chunk::chunk_ticket_storage::ChunkTicketStorage;
    use crate::chunk::player_ticket_tracker::PlayerTicketTracker;
    use uuid::Uuid;

    let mut storage = ChunkTicketStorage::new();
    let mut players = PlayerTicketTracker::new(2, 2);
    let mut manager = LoadTicketManager::new();
    let mut previous_levels = FxHashMap::default();
    for (id, pos) in [(1, ChunkPos::new(0, 0)), (2, ChunkPos::new(3, 0))] {
        let changes = players.add_player(&mut storage, pos, Uuid::from_u128(id));
        manager.apply_source_updates(
            changes
                .load_positions
                .into_iter()
                .map(|pos| storage.load_source_update(pos)),
        );
    }
    run_and_compare_with_reference(&mut manager, &mut previous_levels);

    for pos in [
        ChunkPos::new(1, 0),
        ChunkPos::new(2, 1),
        ChunkPos::new(50, -50),
    ] {
        let changes = players.add_player(&mut storage, pos, Uuid::from_u128(1));
        manager.apply_source_updates(
            changes
                .load_positions
                .into_iter()
                .map(|pos| storage.load_source_update(pos)),
        );
        run_and_compare_with_reference(&mut manager, &mut previous_levels);
    }
    let changes = players.remove_player(&mut storage, ChunkPos::new(3, 0), Uuid::from_u128(2));
    manager.apply_source_updates(
        changes
            .load_positions
            .into_iter()
            .map(|pos| storage.load_source_update(pos)),
    );
    run_and_compare_with_reference(&mut manager, &mut previous_levels);
    assert_eq!(manager.get_level(ChunkPos::new(3, 0)), None);
}
