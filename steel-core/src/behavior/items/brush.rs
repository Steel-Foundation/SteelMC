use std::sync::Arc;

use glam::DVec3;
use steel_macros::item_behavior;
use steel_registry::item_stack::ItemStack;
use steel_registry::particle_type::{BlockParticleOption, ParticleData};
use steel_registry::sound_events;
use steel_registry::vanilla_particle_types;
use steel_utils::Downcast as _;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, Direction};

use crate::behavior::context::{InteractionResult, UseOnContext};
use crate::behavior::{BLOCK_BEHAVIORS, ItemBehavior, UseAnimation};
use crate::block_entity::entities::BrushableBlockEntity;
use crate::entity::{Entity as _, LivingEntity};
use crate::player::{HumanoidArm, Player};
use crate::world::{ClipBlockShape, ClipFluid, World};

const USE_DURATION: i32 = 200;
const ANIMATION_DURATION: i32 = 10;
const SOUND_TICK_OFFSET: i32 = 5;

/// Vanilla brush item behavior for continuous archaeology brushing.
#[item_behavior]
pub struct BrushItem;

impl ItemBehavior for BrushItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        if calculate_hit_result(context.world, context.player).is_none() {
            return InteractionResult::Pass;
        }

        context.player.start_using_item(context.hand);
        InteractionResult::Consume
    }

    fn get_use_duration(&self, _stack: &ItemStack, _user: &dyn LivingEntity) -> i32 {
        USE_DURATION
    }

    fn get_use_animation(&self, _stack: &ItemStack) -> UseAnimation {
        UseAnimation::Brush
    }

    fn on_use_tick(
        &self,
        world: &Arc<World>,
        user: &dyn LivingEntity,
        stack: &mut ItemStack,
        ticks_remaining: i32,
    ) {
        if ticks_remaining < 0 {
            release_player_use(user);
            return;
        }

        let Some(player) = user.as_player() else {
            return;
        };
        let Some((pos, direction, hit_location)) = calculate_hit_result(world, player) else {
            player.release_using_item();
            return;
        };

        let elapsed = USE_DURATION - ticks_remaining + 1;
        if elapsed % ANIMATION_DURATION != SOUND_TICK_OFFSET {
            return;
        }

        let state = world.get_block_state(pos);
        spawn_dust_particles(world, player, state, hit_location, direction);
        let sound = BLOCK_BEHAVIORS
            .get_behavior_for_state(state)
            .and_then(|behavior| behavior.brushable_data(state))
            .map_or(
                &sound_events::ITEM_BRUSH_BRUSHING_GENERIC,
                |(_, sound, _)| sound,
            );
        world.play_block_sound(sound, pos, 1.0, 1.0, Some(player.id()));

        let Some(block_entity) = world.get_block_entity(pos) else {
            return;
        };
        let mut guard = block_entity.lock();
        let Some(brushable) = guard.downcast_mut::<BrushableBlockEntity>() else {
            return;
        };

        if brushable.brush(world.game_time(), world, player, direction, stack) {
            stack.hurt_and_break(1, player.has_infinite_materials());
        }
    }
}

fn calculate_hit_result(world: &World, player: &Player) -> Option<(BlockPos, Direction, DVec3)> {
    let (start, end) = player.get_ray_endpoints();
    let hit = world.clip(start, end, ClipBlockShape::Outline, ClipFluid::None);
    if hit.is_miss() {
        None
    } else {
        Some((hit.block_pos, hit.direction, hit.location))
    }
}

fn release_player_use(user: &dyn LivingEntity) {
    if let Some(player) = user.as_player() {
        player.release_using_item();
    }
}

fn spawn_dust_particles(
    world: &World,
    player: &Player,
    state: steel_utils::BlockStateId,
    hit_location: DVec3,
    hit_direction: Direction,
) {
    let flip = if brushing_arm(player) == HumanoidArm::Right {
        1.0
    } else {
        -1.0
    };
    let (delta_x, delta_z) = dust_particles_delta(player.look_angle(), hit_direction);
    let particle_count = rand::random_range(7..12);
    let particle = ParticleData::new(
        &vanilla_particle_types::BLOCK,
        BlockParticleOption::new(state),
    );
    let pos = DVec3::new(
        hit_location.x
            - if hit_direction == Direction::West {
                1.0e-6
            } else {
                0.0
            },
        hit_location.y,
        hit_location.z
            - if hit_direction == Direction::North {
                1.0e-6
            } else {
                0.0
            },
    );

    // Vanilla uses count 0 so the client treats offset as a single-particle velocity.
    for _ in 0..particle_count {
        let spread = DVec3::new(
            delta_x * flip * 3.0 * rand::random::<f64>(),
            0.0,
            delta_z * flip * 3.0 * rand::random::<f64>(),
        );
        world.send_particles_with_options(particle.clone(), false, false, pos, 0, spread, 1.0);
    }
}

fn brushing_arm(player: &Player) -> HumanoidArm {
    let main_arm = player.main_arm();
    if player.active_item_use_hand() == Some(InteractionHand::MainHand) {
        return main_arm;
    }

    match main_arm {
        HumanoidArm::Left => HumanoidArm::Right,
        HumanoidArm::Right => HumanoidArm::Left,
    }
}

fn dust_particles_delta(view_vector: DVec3, hit_direction: Direction) -> (f64, f64) {
    match hit_direction {
        Direction::Down | Direction::Up => (view_vector.z, -view_vector.x),
        Direction::North => (1.0, -0.1),
        Direction::South => (-1.0, 0.1),
        Direction::West => (-0.1, -1.0),
        Direction::East => (0.1, 1.0),
    }
}
