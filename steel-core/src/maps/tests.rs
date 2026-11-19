use crate::world::World;
use std::sync::Arc;
use steel_registry::data_components::vanilla_components::MapId;
use tokio::runtime::Builder;

use steel_registry::{init_vanilla_registry, vanilla_map_decoration_types};
use steel_utils::Identifier;

use super::*;
use crate::test_support::{TestPlayerBuilder, fresh_test_world, test_domain};

#[test]
fn centers_follow_the_grid_at_negative_edges() {
    for (origin, scale, center) in [
        (63.0, 0, 0),
        (64.0, 0, 128),
        (-64.0, 0, 0),
        (-65.0, 0, -128),
        (0.0, 2, 192),
        (-65.0, 2, -320),
    ] {
        let data = MapItemSavedData::create_fresh(
            origin,
            origin,
            scale,
            true,
            false,
            Identifier::vanilla_static("overworld"),
        );
        assert_eq!((data.center_x, data.center_z), (center, center));
    }
}

#[test]
fn domain_maps_share_ids_across_worlds_and_keep_other_domains_isolated() {
    let worlds = test_domain("map_domain", &["overworld", "the_nether"]);
    let other = test_domain("other_map_domain", &["overworld"]);
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let _maps = DomainMaps::load(&worlds).await.expect("maps");
        let _other_maps = DomainMaps::load(&other).await.expect("other maps");
        let first_world = worlds.default_world("map_domain").expect("default world");
        let second_world = worlds
            .get(&Identifier::new_static("map_domain", "the_nether"))
            .expect("nether");
        let new_data = |world: &World| {
            MapItemSavedData::create_fresh(0.0, 0.0, 0, true, false, world.key.clone())
        };
        let first = first_world.add_map_data(new_data(first_world));
        let second = second_world.add_map_data(new_data(second_world));
        assert_eq!(second.id(), first.id() + 1);
        let first_data = first_world.get_map_data(first).expect("original map");
        assert!(Arc::ptr_eq(
            &first_data,
            &second_world.get_map_data(first).expect("shared map")
        ));
        let other_world = other
            .default_world("other_map_domain")
            .expect("other default world");
        assert!(other_world.get_map_data(first).is_none());
        assert_eq!(other_world.add_map_data(new_data(other_world)), first);
        assert!(!Arc::ptr_eq(
            &first_data,
            &other_world.get_map_data(first).expect("other map")
        ));
    });
}

#[test]
fn holder_updates_bound_changed_pixels_and_throttle_decorations_independently() {
    init_vanilla_registry();
    let fixture = fresh_test_world("map_packets");
    let player = TestPlayerBuilder::new(Arc::clone(&fixture.world), "MapHolder", 1).build();
    let mut data =
        MapItemSavedData::create_fresh(0.0, 0.0, 0, true, false, fixture.world.key.clone());
    data.register_holder(&player);
    let id = MapId::new(17);
    let first = data.update_packet(id, &player).expect("initial update");
    let patch = first.color_patch.expect("initial image");
    assert_eq!(
        (patch.width, patch.height, patch.colors.len()),
        (128, 128, 16_384)
    );
    assert_eq!(first.decorations, Some(Vec::new()));
    assert!(data.update_packet(id, &player).is_none());
    assert!(data.update_color(7, 9, 18));
    assert!(!data.update_color(7, 9, 18));
    data.set_color(11, 12, 23);
    data.add_decoration(
        &vanilla_map_decoration_types::TARGET_X,
        "target".to_owned(),
        7.0,
        9.0,
        180.0,
        None,
        None,
    );
    let update = data.update_packet(id, &player).expect("pixel update");
    assert!(update.decorations.is_none());
    let patch = update.color_patch.expect("bounded pixels");
    assert_eq!(
        (patch.start_x, patch.start_y, patch.width, patch.height),
        (7, 9, 5, 4)
    );
    assert_eq!(patch.colors[0], 18);
    assert_eq!(patch.colors[19], 23);
    for _ in 0..3 {
        assert!(data.update_packet(id, &player).is_none());
    }
    let update = data
        .update_packet(id, &player)
        .expect("fifth dirty-decoration call");
    assert!(update.color_patch.is_none());
    assert_eq!(update.decorations.expect("decorations").len(), 1);
}

#[test]
fn markers_clamp_players_outside_the_map_and_use_nether_rotation_arithmetic() {
    init_vanilla_registry();
    let mut data = MapItemSavedData::create_fresh(
        0.0,
        0.0,
        0,
        true,
        false,
        Identifier::vanilla_static("overworld"),
    );
    let key = "holder".to_owned();
    data.add_decoration(
        &vanilla_map_decoration_types::PLAYER,
        key.clone(),
        64.0,
        -64.0,
        90.0,
        None,
        None,
    );
    let marker = &data.decorations[0].1;
    assert_eq!(
        marker.decoration_type.value().key,
        vanilla_map_decoration_types::PLAYER_OFF_MAP.key
    );
    assert_eq!((marker.x, marker.y, marker.rotation), (127, -128, 4));
    data.add_decoration(
        &vanilla_map_decoration_types::PLAYER,
        key.clone(),
        320.0,
        0.0,
        0.0,
        None,
        None,
    );
    assert_eq!(data.decorations, []);
    data.unlimited_tracking = true;
    data.add_decoration(
        &vanilla_map_decoration_types::PLAYER,
        key,
        320.0,
        0.0,
        0.0,
        Some(100),
        None,
    );
    assert_eq!(
        data.decorations[0].1.decoration_type.value().key,
        vanilla_map_decoration_types::PLAYER_OFF_LIMITS.key
    );
    let value = 10_i32
        .wrapping_mul(10)
        .wrapping_mul(34_187_121)
        .wrapping_add(10 * 121);
    assert_eq!(data.decorations[0].1.rotation, ((value >> 15) & 15) as u8);
}
