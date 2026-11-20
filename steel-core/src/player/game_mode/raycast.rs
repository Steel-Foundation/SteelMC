use super::{ClipBlockShape, ClipFluid, DVec3, World, WorldAabb};

pub(super) fn piercing_ray_hits(
    world: &World,
    bounding_box: WorldAabb,
    from: DVec3,
    to: DVec3,
    entity_margin: f64,
) -> bool {
    if bounding_box.contains(from) || World::clip_entity_box(bounding_box, from, to).is_some() {
        return true;
    }

    if entity_margin <= 0.0 {
        return false;
    }
    let Some(outside) = World::clip_entity_box(bounding_box.inflate(entity_margin), from, to)
    else {
        return false;
    };

    let center = bounding_box.center();
    let block_hit =
        world.clip_including_border(outside, center, ClipBlockShape::Collider, ClipFluid::None);
    World::clip_entity_box(bounding_box, outside, block_hit.location).is_some()
}
