//! Brain Sensors

use super::memory::{Memories, MemoryModuleType};
use crate::entity::PathfinderMob;

mod nearest_living_entities;

pub(crate) use nearest_living_entities::NearestLivingEntitiesSensor;

const DEFAULT_SCAN_RATE: i32 = 20;

pub(crate) trait Sensor: Send {
    fn requires(&self) -> &[MemoryModuleType];

    fn scan_rate(&self) -> i32 {
        DEFAULT_SCAN_RATE
    }

    fn do_tick(&mut self, mob: &dyn PathfinderMob, memories: &mut Memories);
}

pub(crate) struct WrappedSensor {
    scan_rate: i32,
    time_to_tick: i32,
    sensor: Box<dyn Sensor>,
}

impl WrappedSensor {
    #[must_use]
    pub(crate) fn new(sensor: Box<dyn Sensor>) -> Self {
        let scan_rate = sensor.scan_rate();
        Self {
            scan_rate,
            time_to_tick: rand::random_range(0..scan_rate),
            sensor,
        }
    }

    #[must_use]
    pub(crate) fn requires(&self) -> &[MemoryModuleType] {
        self.sensor.requires()
    }

    pub(crate) fn tick(&mut self, mob: &dyn PathfinderMob, memories: &mut Memories){
        if self.should_tick() {
            self.sensor.do_tick(mob, memories);
        }
    }

    const fn should_tick(&mut self) -> bool {
        self.time_to_tick -= 1;
        if self.time_to_tick <= 0 {
            self.time_to_tick = self.scan_rate;
            true
        } else {
            false
        }
    }
}


