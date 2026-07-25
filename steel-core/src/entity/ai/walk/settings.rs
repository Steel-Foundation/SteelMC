use super::{BlockPos, FluidState, Mob, PathType, PathfindingMalus, WorldAabb, fast_floor};

#[derive(Debug, Clone)]
pub struct MobPathSettings {
    entity_width: i32,
    entity_height: i32,
    entity_depth: i32,
    mob_position_vec: glam::DVec3,
    mob_position: BlockPos,
    bounding_box: WorldAabb,
    on_ground: bool,
    in_water: bool,
    can_stand_on_fluid: fn(FluidState) -> bool,
    max_up_step: f32,
    max_fall_distance: i32,
    malus: [f32; PathType::COUNT],
    can_pass_doors: bool,
    can_open_doors: bool,
    can_float: bool,
    can_walk_over_fences: bool,
}

impl MobPathSettings {
    #[must_use]
    pub fn from_mob<M: Mob + ?Sized>(mob: &M) -> Self {
        let bounding_box = mob.bounding_box();
        let mut malus = [0.0; PathType::COUNT];
        for path_type in PathType::ALL {
            malus[path_type.index()] = mob.get_pathfinding_malus(path_type);
        }

        let navigation = mob.mob_base().navigation().lock();
        let can_float = navigation.can_float();
        let can_open_doors = navigation.can_open_doors();
        let can_walk_over_fences = navigation.can_walk_over_fences();
        drop(navigation);

        Self {
            entity_width: fast_floor(bounding_box.width() + 1.0),
            entity_height: fast_floor(bounding_box.height() + 1.0),
            entity_depth: fast_floor(bounding_box.width() + 1.0),
            mob_position_vec: mob.position(),
            mob_position: mob.block_position(),
            bounding_box,
            on_ground: mob.on_ground(),
            in_water: mob.is_in_water(),
            can_stand_on_fluid: |_| false,
            max_up_step: mob.max_up_step(),
            max_fall_distance: mob.max_fall_distance(),
            malus,
            can_pass_doors: true,
            can_open_doors,
            can_float,
            can_walk_over_fences,
        }
    }

    #[must_use]
    pub fn new(
        entity_width: i32,
        entity_height: i32,
        entity_depth: i32,
        mob_position: BlockPos,
        pathfinding_malus: &PathfindingMalus,
    ) -> Self {
        let width = entity_width.max(1);
        let height = entity_height.max(1);
        let depth = entity_depth.max(1);
        let center_x = f64::from(mob_position.x()) + 0.5;
        let center_z = f64::from(mob_position.z()) + 0.5;
        let bounding_box = WorldAabb::new(
            center_x - f64::from(width) * 0.5,
            f64::from(mob_position.y()),
            center_z - f64::from(depth) * 0.5,
            center_x + f64::from(width) * 0.5,
            f64::from(mob_position.y()) + f64::from(height),
            center_z + f64::from(depth) * 0.5,
        );
        let mut malus = [0.0; PathType::COUNT];
        for path_type in PathType::ALL {
            malus[path_type.index()] = pathfinding_malus.get(path_type);
        }

        Self {
            entity_width: width,
            entity_height: height,
            entity_depth: depth,
            mob_position_vec: glam::DVec3::new(center_x, f64::from(mob_position.y()), center_z),
            mob_position,
            bounding_box,
            on_ground: true,
            in_water: false,
            can_stand_on_fluid: |_| false,
            max_up_step: 0.6,
            max_fall_distance: 3,
            malus,
            can_pass_doors: true,
            can_open_doors: false,
            can_float: false,
            can_walk_over_fences: false,
        }
    }

    #[must_use]
    pub const fn with_can_pass_doors(mut self, can_pass_doors: bool) -> Self {
        self.can_pass_doors = can_pass_doors;
        self
    }

    #[must_use]
    pub const fn with_can_open_doors(mut self, can_open_doors: bool) -> Self {
        self.can_open_doors = can_open_doors;
        self
    }

    #[must_use]
    pub const fn with_can_float(mut self, can_float: bool) -> Self {
        self.can_float = can_float;
        self
    }

    #[must_use]
    pub const fn with_can_walk_over_fences(mut self, can_walk_over_fences: bool) -> Self {
        self.can_walk_over_fences = can_walk_over_fences;
        self
    }

    #[must_use]
    pub const fn with_max_up_step(mut self, max_up_step: f32) -> Self {
        self.max_up_step = max_up_step;
        self
    }

    #[must_use]
    pub const fn with_max_fall_distance(mut self, max_fall_distance: i32) -> Self {
        self.max_fall_distance = max_fall_distance;
        self
    }

    #[must_use]
    pub const fn with_on_ground(mut self, on_ground: bool) -> Self {
        self.on_ground = on_ground;
        self
    }

    #[must_use]
    pub const fn with_in_water(mut self, in_water: bool) -> Self {
        self.in_water = in_water;
        self
    }

    #[must_use]
    pub const fn with_can_stand_on_fluid(
        mut self,
        can_stand_on_fluid: fn(FluidState) -> bool,
    ) -> Self {
        self.can_stand_on_fluid = can_stand_on_fluid;
        self
    }

    #[must_use]
    pub const fn entity_width(&self) -> i32 {
        self.entity_width
    }

    #[must_use]
    pub const fn entity_height(&self) -> i32 {
        self.entity_height
    }

    #[must_use]
    pub const fn entity_depth(&self) -> i32 {
        self.entity_depth
    }

    #[must_use]
    pub const fn mob_position(&self) -> BlockPos {
        self.mob_position
    }

    #[must_use]
    pub const fn mob_position_vec(&self) -> glam::DVec3 {
        self.mob_position_vec
    }

    #[must_use]
    pub const fn bounding_box(&self) -> WorldAabb {
        self.bounding_box
    }

    #[must_use]
    pub const fn on_ground(&self) -> bool {
        self.on_ground
    }

    #[must_use]
    pub const fn in_water(&self) -> bool {
        self.in_water
    }

    #[must_use]
    pub fn can_stand_on_fluid(&self, fluid_state: FluidState) -> bool {
        (self.can_stand_on_fluid)(fluid_state)
    }

    #[must_use]
    pub const fn max_up_step(&self) -> f32 {
        self.max_up_step
    }

    #[must_use]
    pub const fn max_fall_distance(&self) -> i32 {
        self.max_fall_distance
    }

    #[must_use]
    pub const fn pathfinding_malus(&self, path_type: PathType) -> f32 {
        self.malus[path_type.index()]
    }

    #[must_use]
    pub const fn can_pass_doors(&self) -> bool {
        self.can_pass_doors
    }

    #[must_use]
    pub const fn can_open_doors(&self) -> bool {
        self.can_open_doors
    }

    #[must_use]
    pub const fn can_float(&self) -> bool {
        self.can_float
    }

    #[must_use]
    pub const fn can_walk_over_fences(&self) -> bool {
        self.can_walk_over_fences
    }
}
