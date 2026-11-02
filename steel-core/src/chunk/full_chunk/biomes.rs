//! Biome cell writes for [`World::fill_biome`](crate::world::World::fill_biome).

use steel_utils::BlockPos;

use super::FullChunkRef;

impl FullChunkRef<'_> {
    /// Sets the biome of every 4×4×4 cell of this chunk that intersects the
    /// inclusive block box `min..=max`, returning how many cells changed.
    pub(crate) fn fill_biome_cells(&self, min: BlockPos, max: BlockPos, biome: u16) -> u64 {
        let min_y = min.0.y.max(self.min_y());
        let max_y = max.0.y.min(self.min_y() + self.height() - 1);
        if min_y > max_y {
            return 0;
        }
        let quart = |block: i32| (block & 15) >> 2;
        let mut changed = 0;
        for section_index in self.get_section_index(min_y)..=self.get_section_index(max_y) {
            let section_min_y = self.min_y() + section_index as i32 * 16;
            let low = min_y.max(section_min_y);
            let high = max_y.min(section_min_y + 15);
            let mut section = self.chunk.sections.sections[section_index].write();
            let mut section_changed = false;
            for cy in quart(low)..=quart(high) {
                for cz in quart(min.0.z)..=quart(max.0.z) {
                    for cx in quart(min.0.x)..=quart(max.0.x) {
                        let (x, y, z) = (cx as usize, cy as usize, cz as usize);
                        if section.biomes.set(x, y, z, biome) != biome {
                            changed += 1;
                            section_changed = true;
                        }
                    }
                }
            }
            if section_changed {
                section.mark_client_biomes_changed();
            }
        }
        if changed > 0 {
            self.mark_unsaved();
        }
        changed
    }
}
