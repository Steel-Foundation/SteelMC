//! Vanilla piston push-structure discovery.

use steel_registry::blocks::behavior::PushReaction;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, Direction};

use super::piston_base_block::PistonBaseBlock;
use crate::world::{LevelReader, World};

const MAX_PUSH_DEPTH: usize = 12;

pub(super) trait PistonLevel: LevelReader {
    fn is_within_world_border(&self, pos: BlockPos) -> bool;
}

impl PistonLevel for World {
    fn is_within_world_border(&self, pos: BlockPos) -> bool {
        self.is_block_within_world_border(pos)
    }
}

pub(super) struct PistonStructureResolver<'a> {
    level: &'a dyn PistonLevel,
    piston_pos: BlockPos,
    extending: bool,
    start_pos: BlockPos,
    push_direction: Direction,
    piston_direction: Direction,
    to_push: Vec<BlockPos>,
    to_destroy: Vec<BlockPos>,
}

impl<'a> PistonStructureResolver<'a> {
    pub(super) fn new(
        level: &'a dyn PistonLevel,
        piston_pos: BlockPos,
        direction: Direction,
        extending: bool,
    ) -> Self {
        let (push_direction, start_pos) = if extending {
            (direction, piston_pos.relative(direction))
        } else {
            (direction.opposite(), piston_pos.relative_n(direction, 2))
        };
        Self {
            level,
            piston_pos,
            extending,
            start_pos,
            push_direction,
            piston_direction: direction,
            to_push: Vec::new(),
            to_destroy: Vec::new(),
        }
    }

    pub(super) fn resolve(&mut self) -> bool {
        self.to_push.clear();
        self.to_destroy.clear();
        let next_state = self.level.get_block_state(self.start_pos);
        if !PistonBaseBlock::is_pushable(
            next_state,
            self.level,
            self.start_pos,
            self.push_direction,
            false,
            self.piston_direction,
        ) {
            if self.extending
                && next_state.get_block().config.push_reaction == PushReaction::Destroy
            {
                self.to_destroy.push(self.start_pos);
                return true;
            }
            return false;
        }

        if !self.add_block_line(self.start_pos, self.push_direction) {
            return false;
        }

        let mut index = 0;
        while index < self.to_push.len() {
            let pos = self.to_push[index];
            if Self::is_sticky(self.level.get_block_state(pos)) && !self.add_branching_blocks(pos) {
                return false;
            }
            index += 1;
        }
        true
    }

    fn is_sticky(state: steel_utils::BlockStateId) -> bool {
        state.get_block() == &vanilla_blocks::SLIME_BLOCK
            || state.get_block() == &vanilla_blocks::HONEY_BLOCK
    }

    fn can_stick_to_each_other(
        first: steel_utils::BlockStateId,
        second: steel_utils::BlockStateId,
    ) -> bool {
        let first_block = first.get_block();
        let second_block = second.get_block();
        if (first_block == &vanilla_blocks::HONEY_BLOCK
            && second_block == &vanilla_blocks::SLIME_BLOCK)
            || (first_block == &vanilla_blocks::SLIME_BLOCK
                && second_block == &vanilla_blocks::HONEY_BLOCK)
        {
            return false;
        }
        Self::is_sticky(first) || Self::is_sticky(second)
    }

    fn add_block_line(&mut self, start: BlockPos, direction: Direction) -> bool {
        let mut next_state = self.level.get_block_state(start);
        if next_state.is_air() {
            return true;
        }
        if !PistonBaseBlock::is_pushable(
            next_state,
            self.level,
            start,
            self.push_direction,
            false,
            direction,
        ) || start == self.piston_pos
            || self.to_push.contains(&start)
        {
            return true;
        }

        let mut block_count = 1;
        if block_count + self.to_push.len() > MAX_PUSH_DEPTH {
            return false;
        }

        while Self::is_sticky(next_state) {
            let pos = start.relative_n(self.push_direction.opposite(), block_count as i32);
            let previous_state = next_state;
            next_state = self.level.get_block_state(pos);
            if next_state.is_air()
                || !Self::can_stick_to_each_other(previous_state, next_state)
                || !PistonBaseBlock::is_pushable(
                    next_state,
                    self.level,
                    pos,
                    self.push_direction,
                    false,
                    self.push_direction.opposite(),
                )
                || pos == self.piston_pos
            {
                break;
            }

            block_count += 1;
            if block_count + self.to_push.len() > MAX_PUSH_DEPTH {
                return false;
            }
        }

        let mut blocks_added = 0;
        for offset in (0..block_count).rev() {
            self.to_push
                .push(start.relative_n(self.push_direction.opposite(), offset as i32));
            blocks_added += 1;
        }

        let mut offset = 1;
        loop {
            let pos = start.relative_n(self.push_direction, offset);
            if let Some(collision_pos) = self.to_push.iter().position(|p| *p == pos) {
                self.reorder_list_at_collision(blocks_added, collision_pos);
                let end = collision_pos + blocks_added;
                for index in 0..=end {
                    let branch_pos = self.to_push[index];
                    if Self::is_sticky(self.level.get_block_state(branch_pos))
                        && !self.add_branching_blocks(branch_pos)
                    {
                        return false;
                    }
                }
                return true;
            }

            next_state = self.level.get_block_state(pos);
            if next_state.is_air() {
                return true;
            }
            if !PistonBaseBlock::is_pushable(
                next_state,
                self.level,
                pos,
                self.push_direction,
                true,
                self.push_direction,
            ) || pos == self.piston_pos
            {
                return false;
            }
            if next_state.get_block().config.push_reaction == PushReaction::Destroy {
                self.to_destroy.push(pos);
                return true;
            }
            if self.to_push.len() >= MAX_PUSH_DEPTH {
                return false;
            }

            self.to_push.push(pos);
            blocks_added += 1;
            offset += 1;
        }
    }

    fn reorder_list_at_collision(&mut self, blocks_added: usize, collision_pos: usize) {
        let last_line_start = self.to_push.len() - blocks_added;
        let mut reordered = Vec::with_capacity(self.to_push.len());
        reordered.extend_from_slice(&self.to_push[..collision_pos]);
        reordered.extend_from_slice(&self.to_push[last_line_start..]);
        reordered.extend_from_slice(&self.to_push[collision_pos..last_line_start]);
        self.to_push = reordered;
    }

    fn add_branching_blocks(&mut self, from_pos: BlockPos) -> bool {
        let from_state = self.level.get_block_state(from_pos);
        for direction in Direction::ALL {
            if direction.axis() == self.push_direction.axis() {
                continue;
            }
            let neighbor_pos = from_pos.relative(direction);
            let neighbor_state = self.level.get_block_state(neighbor_pos);
            if Self::can_stick_to_each_other(neighbor_state, from_state)
                && !self.add_block_line(neighbor_pos, direction)
            {
                return false;
            }
        }
        true
    }

    pub(super) const fn push_direction(&self) -> Direction {
        self.push_direction
    }

    pub(super) fn to_push(&self) -> &[BlockPos] {
        &self.to_push
    }

    pub(super) fn to_destroy(&self) -> &[BlockPos] {
        &self.to_destroy
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_blocks;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::TestLevel;

    impl PistonLevel for TestLevel {
        fn is_within_world_border(&self, _pos: BlockPos) -> bool {
            true
        }
    }

    #[test]
    fn resolver_accepts_twelve_blocks_and_rejects_thirteen() {
        init_test_registry();
        init_behaviors();
        let piston_pos = BlockPos::new(0, 64, 0);
        let twelve = TestLevel::default();
        for offset in 1..=12 {
            twelve.set_test_block(
                piston_pos.relative_n(Direction::East, offset),
                vanilla_blocks::STONE.default_state(),
            );
        }
        let mut resolver = PistonStructureResolver::new(&twelve, piston_pos, Direction::East, true);
        assert!(resolver.resolve());
        assert_eq!(resolver.to_push().len(), 12);

        let thirteen = TestLevel::default();
        for offset in 1..=13 {
            thirteen.set_test_block(
                piston_pos.relative_n(Direction::East, offset),
                vanilla_blocks::STONE.default_state(),
            );
        }
        let mut resolver =
            PistonStructureResolver::new(&thirteen, piston_pos, Direction::East, true);
        assert!(!resolver.resolve());
    }

    #[test]
    fn slime_branches_but_does_not_bind_honey() {
        init_test_registry();
        init_behaviors();
        let piston_pos = BlockPos::new(0, 64, 0);
        let slime_pos = piston_pos.east();
        let south_pos = slime_pos.south();
        let honey_pos = slime_pos.north();
        let level = TestLevel::default()
            .with_block(slime_pos, vanilla_blocks::SLIME_BLOCK.default_state())
            .with_block(south_pos, vanilla_blocks::STONE.default_state())
            .with_block(honey_pos, vanilla_blocks::HONEY_BLOCK.default_state());

        let mut resolver = PistonStructureResolver::new(&level, piston_pos, Direction::East, true);
        assert!(resolver.resolve());
        assert!(resolver.to_push().contains(&slime_pos));
        assert!(resolver.to_push().contains(&south_pos));
        assert!(!resolver.to_push().contains(&honey_pos));
    }
}
