use super::*;

use std::sync::OnceLock;

use crate::entity::ai::node::Node;
use crate::entity::entities::mobs::bosses::ender_dragon::flight_graph::{
    NODE_COUNT, OUTER_RING_NODES,
};

/// A fight with at least one crystal alive, which opens the whole graph.
const WITH_CRYSTALS: Option<i32> = Some(1);
/// No fight at all, which confines the dragon to the two inner rings.
const NO_FIGHT: Option<i32> = None;

/// First index of the radius-20 ring. Private to the graph, so restated here.
const INNER_RING_START: usize = 20;

/// A world with chunks out past the radius-60 outer ring, built once and shared.
fn graph_test_world() -> &'static Arc<World> {
    static WORLD: OnceLock<Arc<World>> = OnceLock::new();
    WORLD.get_or_init(|| chunked_test_world("dragon_flight_graph", 4))
}

fn test_graph() -> DragonFlightGraph {
    // The chunks are empty, so every heightmap lookup sits on the world floor and each
    // node clamps to the shared minimum height. That is what makes the ring geometry
    // below deterministic; the chunks only need to exist so the build does not refuse.
    DragonFlightGraph::try_build(graph_test_world()).expect("the arena chunks are loaded")
}

#[test]
fn a_cold_arena_refuses_to_build_a_graph() {
    // The caller caches the graph for the dragon's whole life, so a build off the world
    // floor would pin all 24 nodes to the minimum height permanently.
    assert!(DragonFlightGraph::try_build(test_world()).is_none());
}

#[test]
fn node_zero_sits_on_the_positive_x_axis() {
    let graph = test_graph();
    let node = graph.node(0);

    // Node 0's angle is exactly -2π, so `60 * cos` lands on an integer and the result
    // of `floor` is decided by the trig implementation alone. `f32::cos` would be
    // liable to return 0.99999994 here and yield 59; vanilla's lookup table returns a
    // true 1.0. This is the case that catches a port using the wrong `cos`.
    assert_eq!((node.x, node.z), (60, 0));
}

#[test]
fn every_node_position_is_distinct() {
    let graph = test_graph();

    // The A* compares nodes by index where vanilla compares them by identity, and the
    // two only agree while no two nodes share a position.
    for a in 0..NODE_COUNT {
        for b in (a + 1)..NODE_COUNT {
            let (first, second) = (graph.node(a), graph.node(b));
            assert_ne!(
                (first.x, first.y, first.z),
                (second.x, second.y, second.z),
                "nodes {a} and {b} share a position"
            );
        }
    }
}

#[test]
fn each_ring_sits_at_its_own_radius() {
    let graph = test_graph();

    for index in 0..NODE_COUNT {
        let expected = if index < OUTER_RING_NODES {
            60.0
        } else if index < INNER_RING_START {
            40.0
        } else {
            20.0
        };

        let node = graph.node(index);
        let radius = f64::from(node.x).hypot(f64::from(node.z));
        // Both coordinates are floored independently, so a node can sit up to a full
        // block inside the ring on each axis.
        assert!(
            (radius - expected).abs() < 1.5,
            "node {index} is at radius {radius}, expected {expected}"
        );
    }
}

#[test]
fn nodes_never_sit_below_the_vanilla_floor() {
    let graph = test_graph();

    for index in 0..NODE_COUNT {
        assert_eq!(
            graph.node(index).y,
            73,
            "node {index} ignored the minimum height"
        );
    }
}

#[test]
fn a_path_exists_between_every_pair_of_nodes() {
    let graph = test_graph();

    for start in 0..NODE_COUNT {
        for end in 0..NODE_COUNT {
            let path = graph
                .find_path(start, end, None, WITH_CRYSTALS)
                .unwrap_or_else(|| panic!("no path from node {start} to node {end}"));

            let last = path.end_node().expect("a path always has a final node");
            let target = graph.node(end);
            assert_eq!(
                (last.x, last.y, last.z),
                (target.x, target.y, target.z),
                "the path from {start} to {end} ended somewhere else"
            );
        }
    }
}

#[test]
fn a_path_to_the_current_node_is_a_single_node() {
    let graph = test_graph();

    for index in 0..NODE_COUNT {
        let path = graph
            .find_path(index, index, None, WITH_CRYSTALS)
            .expect("a node can always reach itself");
        assert_eq!(path.node_count(), 1);
    }
}

#[test]
fn a_final_node_is_appended_after_the_graph_nodes() {
    let graph = test_graph();
    let final_node = Node::new(123, 45, -67);

    let path = graph
        .find_path(0, 5, Some(final_node), WITH_CRYSTALS)
        .expect("node 0 can reach node 5");

    // This is how the landing and strafing phases aim at a point off the graph.
    let last = path.end_node().expect("a path always has a final node");
    assert_eq!((last.x, last.y, last.z), (123, 45, -67));
    assert!(path.node_count() > 1);
}

#[test]
fn the_outer_ring_is_skipped_when_no_fight_is_running() {
    let graph = test_graph();
    // Sitting directly on node 0, which is in the outer ring.
    let on_node_zero = DVec3::new(60.0, 73.0, 0.0);

    assert_eq!(graph.closest_node(on_node_zero, WITH_CRYSTALS), 0);

    let restricted = graph.closest_node(on_node_zero, NO_FIGHT);
    assert!(
        restricted >= OUTER_RING_NODES,
        "a fightless dragon picked outer-ring node {restricted}"
    );
}

#[test]
fn a_restricted_path_never_routes_through_the_outer_ring() {
    let graph = test_graph();

    for start in OUTER_RING_NODES..NODE_COUNT {
        for end in OUTER_RING_NODES..NODE_COUNT {
            let path = graph
                .find_path(start, end, None, NO_FIGHT)
                .unwrap_or_else(|| panic!("no restricted path from {start} to {end}"));

            for index in 0..path.node_count() {
                let node = path.node(index).expect("index is within the path");
                let radius = f64::from(node.x).hypot(f64::from(node.z));
                assert!(
                    radius < 50.0,
                    "the restricted path from {start} to {end} used a radius-60 node"
                );
            }
        }
    }
}

#[test]
fn the_closest_node_is_the_nearest_one() {
    let graph = test_graph();

    for index in 0..NODE_COUNT {
        let node = graph.node(index);
        let position = DVec3::new(f64::from(node.x), f64::from(node.y), f64::from(node.z));
        assert_eq!(graph.closest_node(position, WITH_CRYSTALS), index);
    }
}
