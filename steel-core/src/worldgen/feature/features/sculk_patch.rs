#![expect(
    clippy::too_many_arguments,
    reason = "sculk spreading helpers mirror vanilla cursor state"
)]

use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;
use core::mem;
use steel_registry::vanilla_block_entity_types;
use steel_registry::vanilla_block_tags::BlockTag;

const SCULK_DEFAULT_SPREAD_TYPES: [SculkSpreadType; 3] = [
    SculkSpreadType::SamePosition,
    SculkSpreadType::SamePlane,
    SculkSpreadType::WrapAround,
];
const SCULK_SAME_SPACE_SPREAD_TYPES: [SculkSpreadType; 1] = [SculkSpreadType::SamePosition];

#[derive(Clone, Copy)]
enum SculkSpreadType {
    SamePosition,
    SamePlane,
    WrapAround,
}

struct SculkSpreadPos {
    pos: BlockPos,
    face: Direction,
}

#[derive(Clone, Copy)]
enum SculkBehaviorKind {
    Default,
    Sculk,
    SculkVein,
}

struct SculkSpreader {
    is_world_generation: bool,
    replaceable_blocks: Identifier,
    growth_spawn_cost: i32,
    no_growth_radius: i32,
    charge_decay_rate: i32,
    additional_decay_rate: i32,
    cursors: Vec<SculkChargeCursor>,
}

impl SculkSpreader {
    const MAX_CHARGE: i32 = 1000;
    const MAX_CURSORS: usize = 32;

    const fn worldgen() -> Self {
        Self {
            is_world_generation: true,
            replaceable_blocks: BlockTag::SCULK_REPLACEABLE_WORLD_GEN,
            growth_spawn_cost: 50,
            no_growth_radius: 1,
            charge_decay_rate: 5,
            additional_decay_rate: 10,
            cursors: Vec::new(),
        }
    }

    fn add_cursors(&mut self, start_pos: BlockPos, mut charge: i32) {
        while charge > 0 {
            let current_charge = charge.min(Self::MAX_CHARGE);
            self.add_cursor(SculkChargeCursor::new(start_pos, current_charge));
            charge -= current_charge;
        }
    }

    fn add_cursor(&mut self, cursor: SculkChargeCursor) {
        if self.cursors.len() < Self::MAX_CURSORS {
            self.cursors.push(cursor);
        }
    }

    fn clear(&mut self) {
        self.cursors.clear();
    }
}

struct SculkChargeCursor {
    pos: BlockPos,
    charge: i32,
    update_delay: i32,
    decay_delay: i32,
    facings: Option<Vec<Direction>>,
}

impl SculkChargeCursor {
    const MAX_CURSOR_DISTANCE: i32 = 1024;

    const fn new(pos: BlockPos, charge: i32) -> Self {
        Self {
            pos,
            charge,
            update_delay: 0,
            decay_delay: 1,
            facings: None,
        }
    }
}

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn place_sculk_patch_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &SculkPatchConfiguration,
        origin: BlockPos,
    ) -> bool {
        if !Self::can_sculk_spread_from(region, origin) {
            return false;
        }

        let mut spreader = SculkSpreader::worldgen();
        let total_rounds = config.spread_rounds + config.growth_rounds;
        for round in 0..total_rounds {
            for _ in 0..config.charge_count {
                spreader.add_cursors(origin, config.amount_per_charge);
            }

            let spread_veins = round < config.spread_rounds;
            for _ in 0..config.spread_attempts {
                Self::sculk_update_cursors(
                    region,
                    registry,
                    random,
                    origin,
                    &mut spreader,
                    spread_veins,
                );
            }

            spreader.clear();
        }

        let below = origin.below();
        let below_state = region.block_state(below);
        if random.next_f32() <= config.catalyst_chance
            && shapes::is_offset_shape_full_block(below_state.get_collision_shape_at(below))
        {
            let catalyst = vanilla_blocks::SCULK_CATALYST.default_state();
            if region.set_block_state(origin, catalyst, UpdateFlags::UPDATE_ALL) {
                Self::set_empty_block_entity(
                    region,
                    origin,
                    &vanilla_block_entity_types::SCULK_CATALYST,
                    catalyst,
                );
            }
        }

        let extra_growths = config.extra_rare_growths.sample(random);
        for _ in 0..extra_growths {
            let candidate = origin.offset(
                random.next_i32_bounded(5) - 2,
                0,
                random.next_i32_bounded(5) - 2,
            );
            let below = candidate.below();
            if !region.block_state(candidate).is_air()
                || !region
                    .block_state(below)
                    .is_face_sturdy_at(below, Direction::Up)
            {
                continue;
            }

            let shrieker = vanilla_blocks::SCULK_SHRIEKER
                .default_state()
                .set_value(&BlockStateProperties::CAN_SUMMON, true);
            if region.set_block_state(candidate, shrieker, UpdateFlags::UPDATE_ALL) {
                Self::set_empty_block_entity(
                    region,
                    candidate,
                    &vanilla_block_entity_types::SCULK_SHRIEKER,
                    shrieker,
                );
            }
        }

        true
    }

    fn can_sculk_spread_from(region: &WorldGenRegion<'_>, origin: BlockPos) -> bool {
        let start = region.block_state(origin);
        if !matches!(Self::sculk_behavior(start), SculkBehaviorKind::Default) {
            return true;
        }

        if !start.is_air()
            && (start.get_block() != &vanilla_blocks::WATER
                || !get_fluid_state_from_block(start).is_source())
        {
            return false;
        }

        Self::VANILLA_DIRECTION_VALUES.iter().any(|direction| {
            let pos = origin.relative(*direction);
            let state = region.block_state(pos);
            shapes::is_offset_shape_full_block(state.get_collision_shape_at(pos))
        })
    }

    fn sculk_update_cursors(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        origin: BlockPos,
        spreader: &mut SculkSpreader,
        spread_veins: bool,
    ) {
        if spreader.cursors.is_empty() {
            return;
        }

        let cursors = mem::take(&mut spreader.cursors);
        for mut cursor in cursors {
            if Self::sculk_cursor_is_pos_unreasonable(cursor.pos, origin) {
                continue;
            }

            Self::sculk_update_cursor(
                region,
                registry,
                random,
                origin,
                spreader,
                spread_veins,
                &mut cursor,
            );
            if cursor.charge > 0 {
                spreader.cursors.push(cursor);
            }
        }
    }

    fn sculk_update_cursor(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        origin: BlockPos,
        spreader: &SculkSpreader,
        spread_veins: bool,
        cursor: &mut SculkChargeCursor,
    ) {
        if cursor.charge <= 0 {
            return;
        }

        if cursor.update_delay > 0 {
            cursor.update_delay -= 1;
            return;
        }

        let mut current_state = region.block_state(cursor.pos);
        let mut behavior = Self::sculk_behavior(current_state);
        if spread_veins
            && Self::sculk_attempt_spread_vein(
                region,
                cursor.pos,
                current_state,
                cursor.facings.as_deref(),
                spreader.is_world_generation,
                behavior,
            )
            && Self::sculk_can_change_block_state_on_spread(behavior)
        {
            current_state = region.block_state(cursor.pos);
            behavior = Self::sculk_behavior(current_state);
        }

        cursor.charge = Self::sculk_attempt_use_charge(
            region,
            registry,
            random,
            origin,
            spreader,
            spread_veins,
            cursor,
            behavior,
        );
        if cursor.charge <= 0 {
            Self::sculk_on_discharged(region, current_state, cursor.pos);
            return;
        }

        let transfer_pos = Self::sculk_get_valid_movement_pos(region, cursor.pos, random);
        if let Some(transfer_pos) = transfer_pos {
            Self::sculk_on_discharged(region, current_state, cursor.pos);
            cursor.pos = transfer_pos;
            if spreader.is_world_generation
                && !Self::sculk_horizontal_close_to_origin(cursor.pos, origin, 15.0)
            {
                cursor.charge = 0;
                return;
            }
            current_state = region.block_state(transfer_pos);
        }

        if !matches!(
            Self::sculk_behavior(current_state),
            SculkBehaviorKind::Default
        ) {
            cursor.facings = Some(Self::sculk_available_faces(current_state));
        }

        cursor.decay_delay = Self::sculk_update_decay_delay(behavior, cursor.decay_delay);
        cursor.update_delay = Self::sculk_spread_delay(behavior);
    }

    fn sculk_attempt_spread_vein(
        region: &mut WorldGenRegion<'_>,

        pos: BlockPos,
        state: BlockStateId,
        facings: Option<&[Direction]>,
        post_process: bool,
        behavior: SculkBehaviorKind,
    ) -> bool {
        match behavior {
            SculkBehaviorKind::Default => match facings {
                None => Self::sculk_vein_spread_all(region, state, pos, post_process, true) > 0,
                Some(faces) if !faces.is_empty() => {
                    if !state.is_air() && !get_fluid_state_from_block(state).is_water() {
                        return false;
                    }
                    Self::sculk_vein_regrow(region, pos, state, faces)
                }
                Some(_) => Self::sculk_vein_spread_all(region, state, pos, post_process, false) > 0,
            },
            SculkBehaviorKind::Sculk | SculkBehaviorKind::SculkVein => {
                Self::sculk_vein_spread_all(region, state, pos, post_process, false) > 0
            }
        }
    }

    fn sculk_attempt_use_charge(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        origin: BlockPos,
        spreader: &SculkSpreader,
        spread_veins: bool,
        cursor: &SculkChargeCursor,
        behavior: SculkBehaviorKind,
    ) -> i32 {
        match behavior {
            SculkBehaviorKind::Default => {
                if cursor.decay_delay > 0 {
                    cursor.charge
                } else {
                    0
                }
            }
            SculkBehaviorKind::Sculk => {
                Self::sculk_block_attempt_use_charge(region, random, origin, spreader, cursor)
            }
            SculkBehaviorKind::SculkVein => Self::sculk_vein_attempt_use_charge(
                region,
                registry,
                random,
                spreader,
                spread_veins,
                cursor,
            ),
        }
    }

    fn sculk_block_attempt_use_charge(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        origin: BlockPos,
        spreader: &SculkSpreader,
        cursor: &SculkChargeCursor,
    ) -> i32 {
        let charge = cursor.charge;
        if charge == 0 || random.next_i32_bounded(spreader.charge_decay_rate) != 0 {
            return charge;
        }

        let is_close_to_catalyst =
            Self::sculk_closer_than(cursor.pos, origin, spreader.no_growth_radius);
        if !is_close_to_catalyst && Self::sculk_can_place_growth(region, cursor.pos) {
            if random.next_i32_bounded(spreader.growth_spawn_cost) < charge {
                let growth_pos = cursor.pos.above();
                let growth_state = Self::sculk_random_growth_state(
                    region,
                    random,
                    growth_pos,
                    spreader.is_world_generation,
                );
                if region.set_block_state(growth_pos, growth_state, UpdateFlags::UPDATE_ALL) {
                    Self::set_sculk_growth_block_entity(region, growth_pos, growth_state);
                }
            }

            0.max(charge - spreader.growth_spawn_cost)
        } else if random.next_i32_bounded(spreader.additional_decay_rate) != 0 {
            charge
        } else if is_close_to_catalyst {
            charge - 1
        } else {
            charge - Self::sculk_decay_penalty(spreader, cursor.pos, origin, charge)
        }
    }

    fn sculk_vein_attempt_use_charge(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        spreader: &SculkSpreader,
        spread_veins: bool,
        cursor: &SculkChargeCursor,
    ) -> i32 {
        if spread_veins
            && Self::sculk_vein_attempt_place_sculk(region, registry, random, spreader, cursor.pos)
        {
            cursor.charge - 1
        } else if random.next_i32_bounded(spreader.charge_decay_rate) == 0 {
            fast_floor(f64::from(cursor.charge) * 0.5) as i32
        } else {
            cursor.charge
        }
    }

    fn sculk_vein_attempt_place_sculk(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        spreader: &SculkSpreader,
        pos: BlockPos,
    ) -> bool {
        let state = region.block_state(pos);
        let directions = Self::shuffled_directions(random, Self::VANILLA_DIRECTION_VALUES);
        for support in directions {
            if !Self::sculk_vein_has_face(state, support) {
                continue;
            }

            let support_pos = pos.relative(support);
            let support_state = region.block_state(support_pos);
            if !registry
                .blocks
                .is_in_tag(support_state.get_block(), &spreader.replaceable_blocks)
            {
                continue;
            }

            let sculk = vanilla_blocks::SCULK.default_state();
            let _ = region.set_block_state(support_pos, sculk, UpdateFlags::UPDATE_ALL);
            let _ = Self::sculk_vein_spread_all(
                region,
                sculk,
                support_pos,
                spreader.is_world_generation,
                false,
            );

            let skip = support.opposite();
            for direction in Self::VANILLA_DIRECTION_VALUES {
                if direction == skip {
                    continue;
                }

                let vein_pos = support_pos.relative(direction);
                let possible_vein = region.block_state(vein_pos);
                if possible_vein.get_block() == &vanilla_blocks::SCULK_VEIN {
                    Self::sculk_on_discharged(region, possible_vein, vein_pos);
                }
            }

            return true;
        }

        false
    }

    fn sculk_vein_spread_all(
        region: &mut WorldGenRegion<'_>,

        state: BlockStateId,
        pos: BlockPos,
        post_process: bool,
        same_space_only: bool,
    ) -> i64 {
        let mut count = 0;
        for starting_face in Self::VANILLA_DIRECTION_VALUES {
            if !Self::sculk_vein_can_spread_from(state, starting_face) {
                continue;
            }

            for spread_direction in Self::VANILLA_DIRECTION_VALUES {
                if Self::sculk_vein_spread_from_face_toward_direction(
                    region,
                    state,
                    pos,
                    starting_face,
                    spread_direction,
                    post_process,
                    same_space_only,
                )
                .is_some()
                {
                    count += 1;
                }
            }
        }
        count
    }

    fn sculk_vein_spread_from_face_toward_direction(
        region: &mut WorldGenRegion<'_>,

        state: BlockStateId,
        pos: BlockPos,
        starting_face: Direction,
        spread_direction: Direction,
        post_process: bool,
        same_space_only: bool,
    ) -> Option<SculkSpreadPos> {
        let spread_pos = Self::sculk_vein_get_spread_from_face_toward_direction(
            region,
            state,
            pos,
            starting_face,
            spread_direction,
            same_space_only,
        )?;
        if Self::sculk_vein_spread_to_face(region, &spread_pos, post_process) {
            Some(spread_pos)
        } else {
            None
        }
    }

    fn sculk_vein_get_spread_from_face_toward_direction(
        region: &WorldGenRegion<'_>,

        state: BlockStateId,
        pos: BlockPos,
        starting_face: Direction,
        spread_direction: Direction,
        same_space_only: bool,
    ) -> Option<SculkSpreadPos> {
        if spread_direction.axis() == starting_face.axis() {
            return None;
        }

        if !Self::sculk_vein_is_other_block_valid_as_source(state)
            && (!Self::sculk_vein_has_face(state, starting_face)
                || Self::sculk_vein_has_face(state, spread_direction))
        {
            return None;
        }

        let spread_types = if same_space_only {
            SCULK_SAME_SPACE_SPREAD_TYPES.as_slice()
        } else {
            SCULK_DEFAULT_SPREAD_TYPES.as_slice()
        };
        for spread_type in spread_types {
            let spread_pos =
                Self::sculk_vein_spread_pos(pos, spread_direction, starting_face, *spread_type);
            if Self::sculk_vein_can_spread_into(region, pos, &spread_pos) {
                return Some(spread_pos);
            }
        }

        None
    }

    fn sculk_vein_spread_to_face(
        region: &mut WorldGenRegion<'_>,

        spread_pos: &SculkSpreadPos,
        post_process: bool,
    ) -> bool {
        let old_state = region.block_state(spread_pos.pos);
        let Some(spread_state) = Self::sculk_vein_state_for_placement(
            region,
            old_state,
            spread_pos.pos,
            spread_pos.face,
        ) else {
            return false;
        };

        if post_process {
            region.mark_pos_for_postprocessing(spread_pos.pos);
        }
        region.set_block_state(spread_pos.pos, spread_state, UpdateFlags::UPDATE_CLIENTS)
    }

    fn sculk_vein_can_spread_into(
        region: &WorldGenRegion<'_>,
        source_pos: BlockPos,
        spread_pos: &SculkSpreadPos,
    ) -> bool {
        let existing_state = region.block_state(spread_pos.pos);
        Self::sculk_patch_vein_state_can_be_replaced(
            region,
            source_pos,
            spread_pos.pos,
            spread_pos.face,
            existing_state,
        ) && Self::sculk_vein_is_valid_state_for_placement(
            region,
            existing_state,
            spread_pos.pos,
            spread_pos.face,
        )
    }

    fn sculk_patch_vein_state_can_be_replaced(
        region: &WorldGenRegion<'_>,
        source_pos: BlockPos,
        placement_pos: BlockPos,
        placement_direction: Direction,
        existing_state: BlockStateId,
    ) -> bool {
        let against_state = region.block_state(placement_pos.relative(placement_direction));
        if against_state.get_block() == &vanilla_blocks::SCULK
            || against_state.get_block() == &vanilla_blocks::SCULK_CATALYST
            || against_state.get_block() == &vanilla_blocks::MOVING_PISTON
        {
            return false;
        }

        if Self::manhattan_distance(source_pos, placement_pos) == 2 {
            let neighbor_pos = source_pos.relative(placement_direction.opposite());
            if region
                .block_state(neighbor_pos)
                .is_face_sturdy_at(neighbor_pos, placement_direction)
            {
                return false;
            }
        }

        let fluid_state = get_fluid_state_from_block(existing_state);
        if !fluid_state.is_empty() && !fluid_state.is_water() {
            return false;
        }

        if existing_state.get_block().has_tag(&BlockTag::FIRE) {
            return false;
        }

        existing_state.is_replaceable()
            || Self::sculk_default_multiface_state_can_be_replaced_for_block(existing_state)
    }

    fn sculk_default_multiface_state_can_be_replaced_for_block(
        existing_state: BlockStateId,
    ) -> bool {
        existing_state.is_air()
            || existing_state.get_block() == &vanilla_blocks::SCULK_VEIN
            || (existing_state.get_block() == &vanilla_blocks::WATER
                && get_fluid_state_from_block(existing_state).is_source())
    }

    fn sculk_vein_state_for_placement(
        region: &WorldGenRegion<'_>,
        old_state: BlockStateId,
        placement_pos: BlockPos,
        placement_direction: Direction,
    ) -> Option<BlockStateId> {
        if !Self::sculk_vein_is_valid_state_for_placement(
            region,
            old_state,
            placement_pos,
            placement_direction,
        ) {
            return None;
        }

        let mut new_state = if old_state.get_block() == &vanilla_blocks::SCULK_VEIN {
            old_state
        } else {
            let state = vanilla_blocks::SCULK_VEIN.default_state();
            let fluid_state = get_fluid_state_from_block(old_state);
            if fluid_state.is_water() && fluid_state.is_source() {
                state.set_value(&BlockStateProperties::WATERLOGGED, true)
            } else {
                state
            }
        };
        new_state = new_state.set_value(Self::sculk_vein_face_property(placement_direction), true);
        Some(new_state)
    }

    fn sculk_vein_is_valid_state_for_placement(
        region: &WorldGenRegion<'_>,
        old_state: BlockStateId,
        placement_pos: BlockPos,
        placement_direction: Direction,
    ) -> bool {
        if old_state.get_block() == &vanilla_blocks::SCULK_VEIN
            && Self::sculk_vein_has_face(old_state, placement_direction)
        {
            return false;
        }

        Self::can_attach_to_multiface(region, placement_pos, placement_direction)
    }

    fn sculk_vein_regrow(
        region: &mut WorldGenRegion<'_>,
        pos: BlockPos,
        existing_state: BlockStateId,
        faces: &[Direction],
    ) -> bool {
        let mut has_face = false;
        let mut new_state = vanilla_blocks::SCULK_VEIN.default_state();

        for &face in faces {
            if Self::can_attach_to_multiface(region, pos, face) {
                new_state = new_state.set_value(Self::sculk_vein_face_property(face), true);
                has_face = true;
            }
        }

        if !has_face {
            return false;
        }

        if !get_fluid_state_from_block(existing_state).is_empty() {
            new_state = new_state.set_value(&BlockStateProperties::WATERLOGGED, true);
        }

        region.set_block_state(pos, new_state, UpdateFlags::UPDATE_ALL)
    }

    fn sculk_on_discharged(
        region: &mut WorldGenRegion<'_>,
        mut state: BlockStateId,
        pos: BlockPos,
    ) {
        if state.get_block() != &vanilla_blocks::SCULK_VEIN {
            return;
        }

        for direction in Self::VANILLA_DIRECTION_VALUES {
            if Self::sculk_vein_has_face(state, direction)
                && region.block_state(pos.relative(direction)).get_block() == &vanilla_blocks::SCULK
            {
                state = state.set_value(Self::sculk_vein_face_property(direction), false);
            }
        }

        if !Self::sculk_vein_has_any_face(state) {
            state = if get_fluid_state_from_block(state).is_empty() {
                vanilla_blocks::AIR.default_state()
            } else {
                vanilla_blocks::WATER.default_state()
            };
        }

        let _ = region.set_block_state(pos, state, UpdateFlags::UPDATE_ALL);
    }

    fn sculk_get_valid_movement_pos(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        random: &mut WorldgenRandom,
    ) -> Option<BlockPos> {
        let mut sculk_position = pos;
        for offset in Self::sculk_randomized_non_corner_neighbor_offsets(random) {
            let neighbor = pos.offset(offset.x(), offset.y(), offset.z());
            let transferee = region.block_state(neighbor);
            if matches!(Self::sculk_behavior(transferee), SculkBehaviorKind::Default)
                || !Self::sculk_is_movement_unobstructed(region, pos, neighbor)
            {
                continue;
            }

            sculk_position = neighbor;
            if Self::sculk_vein_has_substrate_access(region, transferee, neighbor) {
                break;
            }
        }

        if sculk_position == pos {
            None
        } else {
            Some(sculk_position)
        }
    }

    fn sculk_randomized_non_corner_neighbor_offsets(random: &mut WorldgenRandom) -> Vec<BlockPos> {
        let mut offsets = Vec::with_capacity(18);
        for z in -1..=1 {
            for y in -1..=1 {
                for x in -1..=1 {
                    if (x == 0 || y == 0 || z == 0) && (x != 0 || y != 0 || z != 0) {
                        offsets.push(BlockPos::new(x, y, z));
                    }
                }
            }
        }

        for i in (1..offsets.len()).rev() {
            let Ok(bound) = i32::try_from(i + 1) else {
                panic!("sculk neighbor offset count exceeds i32 range");
            };
            let j = random.next_i32_bounded(bound) as usize;
            offsets.swap(i, j);
        }
        offsets
    }

    fn sculk_is_movement_unobstructed(
        region: &WorldGenRegion<'_>,
        from: BlockPos,
        to: BlockPos,
    ) -> bool {
        if Self::manhattan_distance(from, to) == 1 {
            return true;
        }

        let dx = to.x() - from.x();
        let dy = to.y() - from.y();
        let dz = to.z() - from.z();
        let direction_x = Self::sculk_direction_from_axis_delta(Axis::X, dx);
        let direction_y = Self::sculk_direction_from_axis_delta(Axis::Y, dy);
        let direction_z = Self::sculk_direction_from_axis_delta(Axis::Z, dz);
        if dx == 0 {
            Self::sculk_is_unobstructed(region, from, direction_y)
                || Self::sculk_is_unobstructed(region, from, direction_z)
        } else if dy == 0 {
            Self::sculk_is_unobstructed(region, from, direction_x)
                || Self::sculk_is_unobstructed(region, from, direction_z)
        } else {
            Self::sculk_is_unobstructed(region, from, direction_x)
                || Self::sculk_is_unobstructed(region, from, direction_y)
        }
    }

    fn sculk_is_unobstructed(
        region: &WorldGenRegion<'_>,
        from: BlockPos,
        direction: Direction,
    ) -> bool {
        let test_pos = from.relative(direction);
        !region
            .block_state(test_pos)
            .is_face_sturdy_at(test_pos, direction.opposite())
    }

    const fn sculk_direction_from_axis_delta(axis: Axis, delta: i32) -> Direction {
        match (axis, delta < 0) {
            (Axis::X, true) => Direction::West,
            (Axis::X, false) => Direction::East,
            (Axis::Y, true) => Direction::Down,
            (Axis::Y, false) => Direction::Up,
            (Axis::Z, true) => Direction::North,
            (Axis::Z, false) => Direction::South,
        }
    }

    fn sculk_vein_has_substrate_access(
        region: &WorldGenRegion<'_>,

        state: BlockStateId,
        pos: BlockPos,
    ) -> bool {
        if state.get_block() != &vanilla_blocks::SCULK_VEIN {
            return false;
        }

        Self::VANILLA_DIRECTION_VALUES.iter().any(|&direction| {
            Self::sculk_vein_has_face(state, direction)
                && region
                    .block_state(pos.relative(direction))
                    .get_block()
                    .has_tag(&BlockTag::SCULK_REPLACEABLE)
        })
    }

    fn sculk_can_place_growth(region: &WorldGenRegion<'_>, pos: BlockPos) -> bool {
        let above = pos.above();
        let state_above = region.block_state(above);
        if !state_above.is_air()
            && (state_above.get_block() != &vanilla_blocks::WATER
                || !get_fluid_state_from_block(state_above).is_water())
        {
            return false;
        }

        let mut growth_count = 0;
        for z in -4..=4 {
            for y in 0..=2 {
                for x in -4..=4 {
                    let state = region.block_state(pos.offset(x, y, z));
                    if state.get_block() == &vanilla_blocks::SCULK_SENSOR
                        || state.get_block() == &vanilla_blocks::SCULK_SHRIEKER
                    {
                        growth_count += 1;
                    }

                    if growth_count > 2 {
                        return false;
                    }
                }
            }
        }

        true
    }

    fn sculk_random_growth_state(
        region: &WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        pos: BlockPos,
        is_world_generation: bool,
    ) -> BlockStateId {
        let state = if random.next_i32_bounded(11) == 0 {
            vanilla_blocks::SCULK_SHRIEKER
                .default_state()
                .set_value(&BlockStateProperties::CAN_SUMMON, is_world_generation)
        } else {
            vanilla_blocks::SCULK_SENSOR.default_state()
        };

        if state
            .try_get_value(&BlockStateProperties::WATERLOGGED)
            .is_some()
            && !get_fluid_state_from_block(region.block_state(pos)).is_empty()
        {
            state.set_value(&BlockStateProperties::WATERLOGGED, true)
        } else {
            state
        }
    }

    fn set_sculk_growth_block_entity(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        state: BlockStateId,
    ) {
        if state.get_block() == &vanilla_blocks::SCULK_SENSOR {
            Self::set_empty_block_entity(
                region,
                pos,
                &vanilla_block_entity_types::SCULK_SENSOR,
                state,
            );
        } else if state.get_block() == &vanilla_blocks::SCULK_SHRIEKER {
            Self::set_empty_block_entity(
                region,
                pos,
                &vanilla_block_entity_types::SCULK_SHRIEKER,
                state,
            );
        }
    }

    fn sculk_decay_penalty(
        spreader: &SculkSpreader,
        pos: BlockPos,
        origin: BlockPos,
        charge: i32,
    ) -> i32 {
        let no_growth_radius = spreader.no_growth_radius as f32;
        let dx = (pos.x() - origin.x()) as f32;
        let dy = (pos.y() - origin.y()) as f32;
        let dz = (pos.z() - origin.z()) as f32;
        let distance = (dx * dx + dy * dy + dz * dz).sqrt();
        let outer_distance_squared = (distance - no_growth_radius) * (distance - no_growth_radius);
        let max_reach = (24 - spreader.no_growth_radius) as f32;
        let max_reach_squared = max_reach * max_reach;
        let distance_factor = (outer_distance_squared / max_reach_squared).min(1.0);
        1.max((charge as f32 * distance_factor * 0.5) as i32)
    }

    fn sculk_closer_than(pos: BlockPos, origin: BlockPos, radius: i32) -> bool {
        let radius_squared = i64::from(radius) * i64::from(radius);
        Self::sculk_distance_squared(pos, origin) < radius_squared
    }

    fn sculk_horizontal_close_to_origin(pos: BlockPos, origin: BlockPos, radius: f64) -> bool {
        let dx = f64::from(pos.x() - origin.x());
        let dz = f64::from(pos.z() - origin.z());
        dx * dx + dz * dz < radius * radius
    }

    fn sculk_distance_squared(left: BlockPos, right: BlockPos) -> i64 {
        let dx = i64::from(left.x()) - i64::from(right.x());
        let dy = i64::from(left.y()) - i64::from(right.y());
        let dz = i64::from(left.z()) - i64::from(right.z());
        dx * dx + dy * dy + dz * dz
    }

    fn sculk_cursor_is_pos_unreasonable(pos: BlockPos, origin: BlockPos) -> bool {
        Self::sculk_abs_diff(pos.x(), origin.x())
            .max(Self::sculk_abs_diff(pos.y(), origin.y()))
            .max(Self::sculk_abs_diff(pos.z(), origin.z()))
            > SculkChargeCursor::MAX_CURSOR_DISTANCE
    }

    const fn sculk_abs_diff(left: i32, right: i32) -> i32 {
        if left >= right {
            left - right
        } else {
            right - left
        }
    }

    fn sculk_update_decay_delay(behavior: SculkBehaviorKind, age: i32) -> i32 {
        match behavior {
            SculkBehaviorKind::Default => (age - 1).max(0),
            SculkBehaviorKind::Sculk | SculkBehaviorKind::SculkVein => 1,
        }
    }

    const fn sculk_spread_delay(_behavior: SculkBehaviorKind) -> i32 {
        1
    }

    const fn sculk_can_change_block_state_on_spread(behavior: SculkBehaviorKind) -> bool {
        !matches!(behavior, SculkBehaviorKind::Sculk)
    }

    fn sculk_behavior(state: BlockStateId) -> SculkBehaviorKind {
        if state.get_block() == &vanilla_blocks::SCULK {
            SculkBehaviorKind::Sculk
        } else if state.get_block() == &vanilla_blocks::SCULK_VEIN {
            SculkBehaviorKind::SculkVein
        } else {
            SculkBehaviorKind::Default
        }
    }

    fn sculk_available_faces(state: BlockStateId) -> Vec<Direction> {
        let mut faces = Vec::new();
        if state.get_block() != &vanilla_blocks::SCULK_VEIN {
            return faces;
        }

        for direction in Self::VANILLA_DIRECTION_VALUES {
            if Self::sculk_vein_has_face(state, direction) {
                faces.push(direction);
            }
        }
        faces
    }

    fn sculk_vein_can_spread_from(state: BlockStateId, face: Direction) -> bool {
        Self::sculk_vein_is_other_block_valid_as_source(state)
            || Self::sculk_vein_has_face(state, face)
    }

    fn sculk_vein_is_other_block_valid_as_source(state: BlockStateId) -> bool {
        state.get_block() != &vanilla_blocks::SCULK_VEIN
    }

    fn sculk_vein_spread_pos(
        pos: BlockPos,
        spread_direction: Direction,
        from_face: Direction,
        spread_type: SculkSpreadType,
    ) -> SculkSpreadPos {
        match spread_type {
            SculkSpreadType::SamePosition => SculkSpreadPos {
                pos,
                face: spread_direction,
            },
            SculkSpreadType::SamePlane => SculkSpreadPos {
                pos: pos.relative(spread_direction),
                face: from_face,
            },
            SculkSpreadType::WrapAround => SculkSpreadPos {
                pos: pos.relative(spread_direction).relative(from_face),
                face: spread_direction.opposite(),
            },
        }
    }

    fn sculk_vein_has_any_face(state: BlockStateId) -> bool {
        Self::VANILLA_DIRECTION_VALUES
            .iter()
            .any(|&direction| Self::sculk_vein_has_face(state, direction))
    }

    fn sculk_vein_has_face(state: BlockStateId, direction: Direction) -> bool {
        state
            .try_get_value(Self::sculk_vein_face_property(direction))
            .unwrap_or(false)
    }

    const fn sculk_vein_face_property(direction: Direction) -> &'static BoolProperty {
        match direction {
            Direction::Up => &BlockStateProperties::UP,
            Direction::Down => &BlockStateProperties::DOWN,
            Direction::North => &BlockStateProperties::NORTH,
            Direction::South => &BlockStateProperties::SOUTH,
            Direction::East => &BlockStateProperties::EAST,
            Direction::West => &BlockStateProperties::WEST,
        }
    }
}
