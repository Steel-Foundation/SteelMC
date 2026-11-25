use super::{ClipBlockShape, ClipFluid, DVec3, World, WorldAabb};

/// Returns whether a piercing ray from `from` to `to` hits `bounding_box`, following
/// vanilla `ProjectileUtil.getManyEntityHitResult` with `includeFromEntity` set.
///
/// A miss on the exact box may still hit through `entity_margin` when the box surface
/// is reachable from the forward hit on the inflated box without crossing a block or
/// the world border.
pub(super) fn piercing_ray_hits(
    world: &World,
    bounding_box: WorldAabb,
    from: DVec3,
    to: DVec3,
    entity_margin: f64,
) -> bool {
    if bounding_box.contains(from) || aabb_clip(bounding_box, from, to).is_some() {
        return true;
    }
    if entity_margin <= 0.0 {
        return false;
    }

    let Some(outside_hit) = aabb_clip(bounding_box.inflate(entity_margin), from, to) else {
        return false;
    };
    let mut towards_target = bounding_box.center();
    let block_hit = world.clip_including_border(
        outside_hit,
        towards_target,
        ClipBlockShape::Collider,
        ClipFluid::None,
    );
    if !block_hit.is_miss() {
        towards_target = block_hit.location;
    }
    aabb_clip(bounding_box, outside_hit, towards_target).is_some()
}

/// Vanilla `AABB.clip`: the first face entered strictly ahead of `from`, so a ray
/// starting inside the box misses.
fn aabb_clip(aabb: WorldAabb, from: DVec3, to: DVec3) -> Option<DVec3> {
    World::intersects_aabb_with_t(from, to, aabb.min_corner(), aabb.max_corner())
        .map(|(scale, _)| from + (to - from) * scale)
}
