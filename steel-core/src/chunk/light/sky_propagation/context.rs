use super::{
    ChunkPos, LightCacheLayout, LightLayer, LightLayerEdit, LightSectionReadCache,
    PackedLightPropagationQueues, SectionPos, SkyLightPropagationContextError,
};

/// ScalableLux-style sky-light propagation over scoped Steel light caches.
pub struct SkyLightPropagationContext<'a, 'sections, 'light> {
    pub(super) layout: LightCacheLayout,
    pub(super) sections: &'a LightSectionReadCache<'sections>,
    pub(super) light: &'a mut LightLayerEdit<'light>,
    pub(super) queues: &'a mut PackedLightPropagationQueues,
    pub(super) missing_section_checked: Vec<bool>,
}

impl<'a, 'sections, 'light> SkyLightPropagationContext<'a, 'sections, 'light> {
    /// Creates a sky-light propagation context from matching scoped caches.
    pub fn new(
        sections: &'a LightSectionReadCache<'sections>,
        light: &'a mut LightLayerEdit<'light>,
        queues: &'a mut PackedLightPropagationQueues,
    ) -> Result<Self, SkyLightPropagationContextError> {
        if light.layer() != LightLayer::Sky {
            return Err(SkyLightPropagationContextError::WrongLayer {
                layer: light.layer(),
            });
        }

        if sections.layout() != light.layout() {
            return Err(SkyLightPropagationContextError::layout_mismatch(
                sections.layout(),
                light.layout(),
            ));
        }

        let layout = light.layout();
        let section_count = layout.range().section_count();

        Ok(Self {
            layout,
            sections,
            light,
            queues,
            missing_section_checked: vec![false; section_count],
        })
    }

    /// Initializes the sky sections required around non-empty center sections.
    pub fn handle_unlit_empty_section_changes(&mut self, chunk_pos: ChunkPos) {
        self.initialize_chunk_sections(chunk_pos, true);
        self.deinit_and_lazy_init_empty_sections(chunk_pos, true);
    }

    /// Synchronizes sky sections for an already-lit loaded chunk without resetting light data.
    pub fn handle_loaded_empty_section_changes(&mut self, chunk_pos: ChunkPos) {
        self.initialize_chunk_sections(chunk_pos, false);
        self.deinit_and_lazy_init_empty_sections(chunk_pos, false);
    }

    fn initialize_chunk_sections(&mut self, chunk_pos: ChunkPos, unlit: bool) {
        for section_y in (self.layout.range().min_chunk_section_y()
            ..self.layout.range().max_chunk_section_y_exclusive())
            .rev()
        {
            let section_pos = SectionPos::new(chunk_pos.0.x, section_y, chunk_pos.0.y);
            if !self.section_is_non_empty(section_pos) {
                continue;
            }

            for offset_z in -1..=1 {
                for offset_x in -1..=1 {
                    let extrude = (offset_x | offset_z) != 0 || !unlit;
                    for offset_y in (-1..=1).rev() {
                        self.init_light_section(
                            SectionPos::new(
                                chunk_pos.0.x + offset_x,
                                section_y + offset_y,
                                chunk_pos.0.y + offset_z,
                            ),
                            extrude,
                            false,
                        );
                    }
                }
            }
        }
    }

    pub(super) fn deinit_and_lazy_init_empty_sections(&mut self, chunk_pos: ChunkPos, unlit: bool) {
        for offset_z in -1..=1 {
            for offset_x in -1..=1 {
                let target_chunk =
                    ChunkPos::new(chunk_pos.0.x + offset_x, chunk_pos.0.y + offset_z);

                for section_y in (self.layout.range().min_section_y()
                    ..self.layout.range().max_section_y_exclusive())
                    .rev()
                {
                    let section_pos =
                        SectionPos::new(target_chunk.0.x, section_y, target_chunk.0.y);
                    match self.section_neighborhood_all_empty_if_known(target_chunk, section_y) {
                        Some(true) => {
                            self.light.set_section_missing(section_pos);
                        }
                        Some(false) => {
                            self.init_light_section(
                                section_pos,
                                (offset_x | offset_z) != 0 || !unlit,
                                false,
                            );
                        }
                        None => {
                            if !self.section_neighborhood_all_empty(target_chunk, section_y) {
                                self.init_light_section(
                                    section_pos,
                                    (offset_x | offset_z) != 0 || !unlit,
                                    false,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    fn section_neighborhood_all_empty(&self, chunk_pos: ChunkPos, section_y: i32) -> bool {
        for offset_y in -1..=1 {
            let neighbor_y = section_y + offset_y;
            if neighbor_y < self.layout.range().min_chunk_section_y()
                || neighbor_y >= self.layout.range().max_chunk_section_y_exclusive()
            {
                continue;
            }

            for offset_z in -1..=1 {
                for offset_x in -1..=1 {
                    let section_pos = SectionPos::new(
                        chunk_pos.0.x + offset_x,
                        neighbor_y,
                        chunk_pos.0.y + offset_z,
                    );
                    if let Some(empty) = self.sections.section_empty(section_pos) {
                        if !empty {
                            return false;
                        }
                    } else if let Some(empty) = self.light.section_empty(section_pos) {
                        if !empty {
                            return false;
                        }
                    } else if self.sections.has_non_empty_section(section_pos) {
                        return false;
                    }
                }
            }
        }

        true
    }

    fn section_neighborhood_all_empty_if_known(
        &self,
        chunk_pos: ChunkPos,
        section_y: i32,
    ) -> Option<bool> {
        for offset_y in -1..=1 {
            let neighbor_y = section_y + offset_y;
            if neighbor_y < self.layout.range().min_chunk_section_y()
                || neighbor_y >= self.layout.range().max_chunk_section_y_exclusive()
            {
                continue;
            }

            for offset_z in -1..=1 {
                for offset_x in -1..=1 {
                    let section_pos = SectionPos::new(
                        chunk_pos.0.x + offset_x,
                        neighbor_y,
                        chunk_pos.0.y + offset_z,
                    );
                    let empty = self.sections.section_empty(section_pos)?;
                    if !empty {
                        return Some(false);
                    }
                }
            }
        }

        Some(true)
    }

    /// Resets the center chunk to `ScalableLux`'s fresh all-missing lighting state.
    pub fn reset_center_chunk_sections(&mut self) {
        self.light
            .reset_chunk_sections_to_missing(self.layout.center_chunk());
    }
}
