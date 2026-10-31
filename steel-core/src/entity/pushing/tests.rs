use std::iter::once;
use std::sync::{Arc, atomic::Ordering};

use glam::DVec3;
use steel_registry::vanilla_game_rules::MAX_ENTITY_CRAMMING;
use steel_utils::ChunkPos;

use super::EntityPush;
use super::test_fixtures::{CustomContact, DEFAULT_ENTITIES, Factory, Fixture, player};
use crate::bootstrap::init_globals;
use crate::entity::{Entity, next_entity_id};
use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

#[test]
fn mixed_species_pushes_match_sequential_contacts_and_custom_callbacks() {
    let pushers = DEFAULT_ENTITIES[..6]
        .iter()
        .copied()
        .chain(once(("player", player as Factory)));

    for (label, pusher) in pushers {
        for custom in [false, true] {
            let reference = Fixture::new(pusher, custom);
            let optimized = Fixture::new(pusher, custom);

            for cramming in [0, 24, 64] {
                reference
                    .world
                    .world
                    .set_game_rule(&MAX_ENTITY_CRAMMING, cramming);
                optimized
                    .world
                    .world
                    .set_game_rule(&MAX_ENTITY_CRAMMING, cramming);

                for case in 0..12 {
                    reference.reset(case);
                    optimized.reset(case);

                    let target = &reference.entities[0];
                    let contacts = reference
                        .world
                        .world
                        .get_pushable_entities(target.as_ref(), &target.bounding_box());
                    target
                        .as_living_entity()
                        .expect("reference target")
                        .apply_entity_cramming_damage(&reference.world.world, &contacts);

                    for contact in contacts {
                        contact.push_entity(target.as_ref());
                    }

                    optimized.entities[0]
                        .as_living_entity()
                        .expect("optimized target")
                        .push_entities();

                    optimized.assert_same_state(&reference, label, case);
                }
            }
        }
    }
}

#[test]
fn pushes_match_sequential_contacts_at_the_cramming_boundary() {
    init_globals();

    for count in [24, 25] {
        for cramming in [0, 24, 25] {
            let reference = fresh_test_world("reference_cramming_boundary");
            let optimized = fresh_test_world("optimized_cramming_boundary");
            let mut groups = Vec::new();

            for fixture in [&reference, &optimized] {
                insert_ready_full_chunk(&fixture.world, ChunkPos::new(0, 0));
                fixture.world.set_tick_runs_normally(true);
                fixture.world.set_game_rule(&MAX_ENTITY_CRAMMING, cramming);
                let mut entities = Vec::new();

                for index in 0..count {
                    let position = DVec3::new(
                        1.6 + f64::from(index % 5) * 0.02,
                        64.0,
                        1.6 + f64::from(index / 5) * 0.02,
                    );
                    let entity = DEFAULT_ENTITIES[index as usize % 6].1(
                        &fixture.world,
                        next_entity_id(),
                        position,
                    );

                    // keep the above-limit comparison independent of the cramming roll
                    entity.set_invulnerable(true);
                    fixture
                        .world
                        .try_add_entity(Arc::clone(&entity))
                        .expect("boundary fixture");
                    entities.push(entity);
                }

                groups.push(entities);
            }

            let target = &groups[0][0];
            let contacts = reference
                .world
                .get_pushable_entities(target.as_ref(), &target.bounding_box());
            assert_eq!(contacts.len(), count as usize - 1);
            target
                .as_living_entity()
                .expect("reference target")
                .apply_entity_cramming_damage(&reference.world, &contacts);

            for contact in contacts {
                contact.push_entity(target.as_ref());
            }

            groups[1][0]
                .as_living_entity()
                .expect("optimized target")
                .push_entities();

            for (expected, actual) in groups[0].iter().zip(&groups[1]) {
                assert_eq!(
                    actual.velocity().to_array().map(f64::to_bits),
                    expected.velocity().to_array().map(f64::to_bits),
                    "{count}/{cramming}"
                );
                assert_eq!(
                    actual.needs_velocity_sync(),
                    expected.needs_velocity_sync(),
                    "{count}/{cramming}"
                );
                assert_eq!(
                    actual
                        .as_living_entity()
                        .expect("actual living")
                        .get_health()
                        .to_bits(),
                    expected
                        .as_living_entity()
                        .expect("reference living")
                        .get_health()
                        .to_bits()
                );
            }
        }
    }
}

#[test]
fn bat_keeps_its_empty_push_entities_override() {
    let reference = Fixture::new(DEFAULT_ENTITIES[6].1, false);
    let actual = Fixture::new(DEFAULT_ENTITIES[6].1, false);
    reference.reset(0);
    actual.reset(0);

    actual.entities[0]
        .as_living_entity()
        .expect("bat")
        .push_entities();

    actual.assert_same_state(&reference, "bat", 0);
}

#[test]
fn default_impulses_preserve_bits_and_custom_velocity_accessors() {
    init_globals();
    let world = fresh_test_world("shared_push_impulse");

    for (label, factory) in DEFAULT_ENTITIES {
        let expected = factory(&world.world, next_entity_id(), DVec3::ZERO);
        let actual = factory(&world.world, next_entity_id(), DVec3::ZERO);

        for initial in [
            DVec3::ZERO,
            DVec3::splat(-0.0),
            DVec3::splat(f64::MAX),
            DVec3::splat(f64::MIN_POSITIVE),
        ] {
            for impulse in [
                DVec3::ZERO,
                DVec3::new(0.05, 0.0, -0.05),
                DVec3::splat(f64::MAX),
                DVec3::splat(f64::INFINITY),
                DVec3::splat(f64::NAN),
            ] {
                expected.set_velocity(initial);
                actual.set_velocity(initial);
                expected.clear_velocity_sync();
                actual.clear_velocity_sync();

                if impulse.is_finite() {
                    expected.set_velocity(expected.velocity() + impulse);
                    expected.mark_velocity_sync();
                }

                actual.push_impulse(impulse);

                assert_eq!(
                    actual.velocity().to_array().map(f64::to_bits),
                    expected.velocity().to_array().map(f64::to_bits),
                    "{label}"
                );
                assert_eq!(
                    actual.needs_velocity_sync(),
                    expected.needs_velocity_sync(),
                    "{label}"
                );
            }
        }
    }

    let custom = CustomContact::new(Arc::downgrade(&world.world));
    custom.push_impulse(DVec3::X);
    assert_eq!(custom.velocity_writes.load(Ordering::Relaxed), 1);
    assert_eq!(custom.sync_writes.load(Ordering::Relaxed), 1);
    assert_eq!(custom.velocity(), DVec3::X);
}

// java 25 results from 26.3 Entity.push, compare exact bits
#[test]
fn separation_rounding_matches_java_reference() {
    let edge = f64::from(0.01_f32);
    let cases = [
        (0.0, -0.0, None),
        (edge.next_down(), 0.0, None),
        (
            edge,
            0.0,
            Some([0x3f74_7ae1_48f5_c28f, 0x0000_0000_0000_0000]),
        ),
        (
            edge.next_up(),
            edge.next_up(),
            Some([0x3f74_7ae1_48f5_c28f, 0x3f74_7ae1_48f5_c28f]),
        ),
        (
            -0.0,
            edge,
            Some([0x8000_0000_0000_0000, 0x3f74_7ae1_48f5_c28f]),
        ),
        (
            0.1,
            0.125,
            Some([0x3f8c_f68d_573c_a831, 0x3f92_1a18_5685_e91e]),
        ),
        (
            -0.5,
            0.125,
            Some([0xbfa2_1a18_5685_e91e, 0x3f82_1a18_5685_e91e]),
        ),
        (
            1.0,
            1.0,
            Some([0x3fa9_9999_a000_0000, 0x3fa9_9999_a000_0000]),
        ),
        (
            1.0_f64.next_up(),
            1.0,
            Some([0x3fa9_9999_a000_0002, 0x3fa9_9999_a000_0000]),
        ),
        (
            2.0,
            -0.5,
            Some([0x3fa9_9999_9fff_fffe, 0xbf89_9999_9fff_fffe]),
        ),
        (f64::MIN_POSITIVE, 0.0, None),
        (
            f64::MAX / 8.0,
            2.0,
            Some([0x3fa9_9999_a000_0000, 0x0003_3333_3400_0000]),
        ),
    ];

    for (x, z, expected) in cases {
        let actual = EntityPush::impulse(DVec3::ZERO, DVec3::new(x, 0.0, z)).map(|impulse| {
            assert_eq!(impulse.y.to_bits(), 0);
            [impulse.x.to_bits(), impulse.z.to_bits()]
        });

        assert_eq!(actual, expected, "offset ({x}, {z})");
    }
}
