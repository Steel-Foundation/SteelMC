use super::WorldAabb;

pub trait WalkNodeCollision {
    fn has_collision(&mut self, aabb: WorldAabb) -> bool;
}

impl<F> WalkNodeCollision for F
where
    F: FnMut(WorldAabb) -> bool,
{
    fn has_collision(&mut self, aabb: WorldAabb) -> bool {
        self(aabb)
    }
}
