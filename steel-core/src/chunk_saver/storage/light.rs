use super::{
    ChunkLightData, ChunkLightLayerStorage, ChunkStatus, ChunkStorage, DATA_LAYER_SIZE,
    LightSection, LightSectionData, PersistentLightData, PersistentLightSection,
    homogeneous_packed_light_value,
};

impl ChunkStorage {
    /// Converts chunk-owned light data to persistent format.
    pub(super) fn light_to_persistent(light: &ChunkLightData) -> PersistentLightData {
        PersistentLightData {
            block: Self::light_layer_to_persistent(&light.block),
            sky: Self::light_layer_to_persistent(&light.sky),
        }
    }

    pub(super) fn light_layer_to_persistent(
        layer: &ChunkLightLayerStorage,
    ) -> Vec<PersistentLightSection> {
        layer
            .sections()
            .iter()
            .enumerate()
            .filter_map(|(index, section)| {
                let Ok(section_index) = u32::try_from(index) else {
                    tracing::warn!(
                        index,
                        "Light section index does not fit in persistent format"
                    );
                    return None;
                };

                match section {
                    LightSection::Missing => None,
                    LightSection::Visible(data) => {
                        if data.is_all_zero() {
                            Some(PersistentLightSection::Uninitialized { section_index })
                        } else {
                            Some(PersistentLightSection::Initialized {
                                section_index,
                                data: data.to_bytes().as_ref().to_vec(),
                            })
                        }
                    }
                    LightSection::Internal(data) => {
                        if data.is_all_zero() {
                            None
                        } else {
                            Some(PersistentLightSection::Internal {
                                section_index,
                                data: data.to_bytes().as_ref().to_vec(),
                            })
                        }
                    }
                }
            })
            .collect()
    }

    pub(super) fn persistent_to_light(
        persistent: &PersistentLightData,
        min_y: i32,
        height: i32,
        status: ChunkStatus,
    ) -> ChunkLightData {
        let mut light = ChunkLightData::for_valid_world_height(min_y, height);
        if status < ChunkStatus::Light {
            return light;
        }

        Self::apply_persistent_light_layer(&mut light.block, &persistent.block, "block");
        Self::apply_persistent_light_layer(&mut light.sky, &persistent.sky, "sky");
        light
            .sky
            .fill_loaded_missing_sky_sections_below_data_with_zero();
        light
    }

    pub(super) fn apply_persistent_light_layer(
        layer: &mut ChunkLightLayerStorage,
        persistent: &[PersistentLightSection],
        layer_name: &str,
    ) {
        for section in persistent {
            let Ok(section_index) = usize::try_from(section.section_index()) else {
                tracing::warn!(
                    layer = layer_name,
                    section_index = section.section_index(),
                    "Persisted light section index does not fit this platform"
                );
                continue;
            };

            let Some(target) = layer.sections_mut().get_mut(section_index) else {
                tracing::warn!(
                    layer = layer_name,
                    section_index,
                    "Persisted light section index is outside world light range"
                );
                continue;
            };

            let Some(restored) = Self::persistent_to_light_section(section, layer_name) else {
                continue;
            };
            *target = restored;
        }
    }

    pub(super) fn persistent_to_light_section(
        persistent: &PersistentLightSection,
        layer_name: &str,
    ) -> Option<LightSection> {
        match persistent {
            PersistentLightSection::Uninitialized { .. } => {
                Some(LightSection::visible(LightSectionData::homogeneous(0)))
            }
            PersistentLightSection::Initialized {
                section_index,
                data,
            } => Self::persistent_light_bytes_to_data(data, *section_index, layer_name)
                .map(LightSection::visible),
            PersistentLightSection::Internal {
                section_index,
                data,
            } => {
                let restored =
                    Self::persistent_light_bytes_to_data(data, *section_index, layer_name)?;
                if restored.is_all_zero() {
                    None
                } else {
                    Some(LightSection::internal(restored))
                }
            }
        }
    }

    pub(super) fn persistent_light_bytes_to_data(
        data: &[u8],
        section_index: u32,
        layer_name: &str,
    ) -> Option<LightSectionData> {
        let actual = data.len();
        let bytes = Box::<[u8]>::from(data);
        let result: Result<Box<[u8; DATA_LAYER_SIZE]>, Box<[u8]>> = bytes.try_into();
        let Ok(bytes) = result else {
            tracing::warn!(
                layer = layer_name,
                section_index,
                actual,
                expected = DATA_LAYER_SIZE,
                "Skipping persisted light section with invalid byte length"
            );
            return None;
        };

        if let Some(value) = homogeneous_packed_light_value(&bytes) {
            Some(LightSectionData::homogeneous(value))
        } else {
            Some(LightSectionData::Packed(bytes))
        }
    }
}
