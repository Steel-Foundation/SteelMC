use super::{ClipBlockShape, ClipFluid, DVec3, World, WorldAabb, swap};

pub(super) fn piercing_ray_hit_t(
    world: &World,
    bounding_box: WorldAabb,
    from: DVec3,
    to: DVec3,
    entity_margin: f64,
) -> Option<f64> {
    if let Some(hit_t) = ray_aabb_hit_t(bounding_box, from, to) {
        return Some(hit_t);
    }
    if entity_margin <= 0.0 {
        return None;
    }

    let outside_hit_t = ray_aabb_hit_t(bounding_box.inflate(entity_margin), from, to)?;
    let outside_hit = from + (to - from) * outside_hit_t;
    let mut towards_target = DVec3::new(
        f64::midpoint(bounding_box.min_x(), bounding_box.max_x()),
        f64::midpoint(bounding_box.min_y(), bounding_box.max_y()),
        f64::midpoint(bounding_box.min_z(), bounding_box.max_z()),
    );
    let block_hit = world.clip(
        outside_hit,
        towards_target,
        ClipBlockShape::Collider,
        ClipFluid::None,
    );
    if !block_hit.is_miss() {
        towards_target = block_hit.location;
    }
    ray_aabb_hit_t(bounding_box, outside_hit, towards_target).map(|_| outside_hit_t)
}

fn ray_aabb_hit_t(aabb: WorldAabb, from: DVec3, to: DVec3) -> Option<f64> {
    if aabb.contains(from) {
        return Some(0.0);
    }

    let delta = to - from;
    let mut t_min = 0.0_f64;
    let mut t_max = 1.0_f64;
    if !update_ray_axis(
        from.x,
        delta.x,
        aabb.min_x(),
        aabb.max_x(),
        &mut t_min,
        &mut t_max,
    ) {
        return None;
    }
    if !update_ray_axis(
        from.y,
        delta.y,
        aabb.min_y(),
        aabb.max_y(),
        &mut t_min,
        &mut t_max,
    ) {
        return None;
    }
    if !update_ray_axis(
        from.z,
        delta.z,
        aabb.min_z(),
        aabb.max_z(),
        &mut t_min,
        &mut t_max,
    ) {
        return None;
    }

    Some(t_min)
}

fn update_ray_axis(
    start: f64,
    delta: f64,
    min: f64,
    max: f64,
    t_min: &mut f64,
    t_max: &mut f64,
) -> bool {
    if delta.abs() < f64::EPSILON {
        return start >= min && start <= max;
    }

    let inverse_delta = 1.0 / delta;
    let mut enter = (min - start) * inverse_delta;
    let mut exit = (max - start) * inverse_delta;
    if enter > exit {
        swap(&mut enter, &mut exit);
    }

    *t_min = (*t_min).max(enter);
    *t_max = (*t_max).min(exit);
    *t_min <= *t_max
}
