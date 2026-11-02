//! Reading and painting biomes in a world.
//!
//! Painting follows vanilla `/fillbiome`: whole 4×4×4 cells intersecting the box
//! change, in loaded chunks only, and players see the change with the current
//! tick's block updates.

#[stabby::host_api]
/// Biome bindings on borrowed worlds.
pub mod api {
    use crate::{ApiResult, BlockPos, World};
    #[cfg(feature = "host")]
    use stabby::string::String as ApiString;
    use stabby::{option::Option as StableOption, str::Str, string::String as BiomeKey};
    #[cfg(feature = "host")]
    use steel_registry::{REGISTRY, RegistryExt as _};
    #[cfg(feature = "host")]
    use steel_utils::Identifier;

    impl<'a> World<'a> {
        /// The biome key at a block, with vanilla's zoom fuzzing (`Level.getBiome`),
        /// or none when its chunk is not loaded.
        #[must_use]
        pub fn biome_at(&self, pos: BlockPos) -> StableOption<BiomeKey> {
            self.host()
                .biome_at(steel_utils::BlockPos::new(pos.x, pos.y, pos.z))
                .map(|biome| BiomeKey::from(biome.key.to_string().as_str()))
                .into()
        }

        /// Paints every biome cell intersecting the inclusive box with `biome`,
        /// returning how many cells changed. Fails without changes when a touched
        /// chunk is not loaded.
        pub fn set_biomes(&self, from: BlockPos, to: BlockPos, biome: Str<'_>) -> ApiResult<u64> {
            set_biomes(self, from, to, biome.as_str())
                .map_err(ApiString::from)
                .into()
        }
    }

    #[cfg(feature = "host")]
    fn set_biomes(
        world: &World<'_>,
        from: BlockPos,
        to: BlockPos,
        biome: &str,
    ) -> Result<u64, String> {
        let key = biome.parse::<Identifier>().map_err(ToString::to_string)?;
        let biome = REGISTRY
            .biomes
            .by_key(&key)
            .ok_or_else(|| format!("unknown biome {key}"))?;
        world
            .host()
            .fill_biome(
                steel_utils::BlockPos::new(from.x, from.y, from.z),
                steel_utils::BlockPos::new(to.x, to.y, to.z),
                biome,
            )
            .map_err(|error| error.to_string())
    }
}
