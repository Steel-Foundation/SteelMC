//! Client-only biome variants that show a fluid's water tint.
//!
//! The client biome registry is the server registry followed by one variant per
//! (tinting fluid, server biome) pair. A variant is its source biome with the
//! fluid's water colors. Chunk sends put variants in the biome cells holding
//! that fluid. Server gameplay and saves never see them: they are not entries of
//! the server biome registry.

use steel_utils::Identifier;

use crate::biome::Biome;
use crate::biome::{BiomeRef, BiomeRegistry};
use crate::{Registry, RegistryEntry as _, RegistryExt as _};

use super::{ClientFluid, FluidTint};

const MAX_COLOR: i32 = 0xFF_FFFF;

impl Registry {
    /// Registers an extension biome. It is sent to clients with the vanilla biomes
    /// and saved in chunks by its key, like any registered biome.
    pub fn register_client_biome(&mut self, biome: Biome) -> Result<BiomeRef, String> {
        self.validate_content_key(&biome.key)?;
        if self.biomes.by_key(&biome.key).is_some() {
            return Err(format!("biome {} is already registered", biome.key));
        }
        if biome.id.get().is_some() {
            return Err("a new biome must not have a cached registry ID".into());
        }
        let effects = &biome.effects;
        let colors = [
            Some(effects.fog_color),
            Some(effects.sky_color),
            Some(effects.water_color),
            Some(effects.water_fog_color),
            effects.grass_color,
            effects.foliage_color,
            effects.dry_foliage_color,
        ];
        if let Some(color) = colors
            .into_iter()
            .flatten()
            .find(|color| !(0..=MAX_COLOR).contains(color))
        {
            return Err(format!("biome color {color:#x} is not a 24-bit RGB value"));
        }
        if !biome.temperature.is_finite() || !biome.downfall.is_finite() {
            return Err("biome temperature and downfall must be finite".into());
        }
        if let Some(particle) = &effects.particle {
            if self
                .particle_types
                .by_key(&particle.options.particle_type)
                .is_none()
            {
                return Err(format!(
                    "unknown particle type {}",
                    particle.options.particle_type
                ));
            }
            if !(0.0..=1.0).contains(&particle.probability) {
                return Err("particle probability must be between 0 and 1".into());
            }
        }
        let biome: BiomeRef = Box::leak(Box::new(biome));
        self.biomes.register(biome);
        Ok(biome)
    }
}

/// One client-only tint variant.
#[derive(Debug)]
pub struct TintVariant {
    /// `<fluid namespace>:<fluid path>_tint/<biome namespace>/<biome path>`.
    pub key: Identifier,
    pub source: BiomeRef,
    pub tint: FluidTint,
}

/// The biome list clients receive, computed once when registries freeze.
#[derive(Debug, Default)]
pub struct ClientBiomes {
    server_count: usize,
    /// Ordered by `(tint_index, server biome id)`.
    variants: Vec<TintVariant>,
}

impl ClientBiomes {
    pub(crate) fn build(biomes: &BiomeRegistry, fluids: &[&'static ClientFluid]) -> Self {
        let server: Vec<BiomeRef> = biomes.iter().map(|(_, biome)| biome).collect();
        let mut tinting: Vec<(u16, &ClientFluid)> = fluids
            .iter()
            .filter_map(|fluid| fluid.tint_index.map(|index| (index, *fluid)))
            .collect();
        tinting.sort_by_key(|(index, _)| *index);
        let mut variants = Vec::with_capacity(tinting.len() * server.len());
        for (_, fluid) in tinting {
            // Tint indices are only assigned together with a tint.
            let Some(tint) = fluid.tint else { continue };
            let owner = &fluid.source.key;
            for biome in &server {
                let path = format!(
                    "{}_tint/{}/{}",
                    owner.path,
                    biome.key().namespace,
                    biome.key().path
                );
                variants.push(TintVariant {
                    key: Identifier::new(owner.namespace.clone(), path),
                    source: biome,
                    tint,
                });
            }
        }
        Self {
            server_count: server.len(),
            variants,
        }
    }

    /// Server biome count; client ids below it are the server ids unchanged.
    #[must_use]
    pub const fn server_count(&self) -> usize {
        self.server_count
    }

    /// The number of biomes clients receive.
    #[must_use]
    pub const fn client_count(&self) -> usize {
        self.server_count + self.variants.len()
    }

    /// Client-only variants, in client id order after the server biomes.
    #[must_use]
    pub fn variants(&self) -> &[TintVariant] {
        &self.variants
    }

    /// The client id showing `tint_index`'s tint over server biome `biome_id`.
    #[must_use]
    pub const fn variant_id(&self, biome_id: usize, tint_index: u16) -> usize {
        self.server_count * (1 + tint_index as usize) + biome_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Registry;
    use crate::client_content::{FluidRegistration, FluidSpread};
    use crate::vanilla_fluids;

    fn tinted(registry: &mut Registry, name: &'static str, color: i32) -> Result<(), String> {
        registry.register_client_fluid(FluidRegistration {
            source_key: Identifier::new("biome_test", name),
            flowing_key: Identifier::new("biome_test", format!("{name}_flowing")),
            block_key: Identifier::new("biome_test", name),
            client_source: &vanilla_fluids::WATER,
            tint: Some(FluidTint {
                water_color: color,
                water_fog_color: color,
            }),
            spread: FluidSpread {
                tick_delay: 5,
                drop_off: 1,
                slope_find_distance: 4,
                converts_to_source: false,
            },
            explosion_resistance: 100.0,
            randomly_ticking: false,
            bucket: None,
        })?;
        Ok(())
    }

    #[test]
    fn no_tinting_fluid_means_no_variants() {
        let registry = Registry::new_vanilla();
        let table = ClientBiomes::build(&registry.biomes, registry.client_content.fluids());
        assert_eq!(table.server_count(), registry.biomes.len());
        assert_eq!(table.client_count(), registry.biomes.len());
    }

    #[test]
    fn each_tinting_fluid_adds_one_variant_per_biome_in_id_order() -> Result<(), String> {
        let mut registry = Registry::new_vanilla();
        tinted(&mut registry, "green", 0x00_FF00)?;
        tinted(&mut registry, "red", 0xFF_0000)?;
        let table = ClientBiomes::build(&registry.biomes, registry.client_content.fluids());
        let count = registry.biomes.len();
        assert_eq!(table.client_count(), count * 3);
        for (id, biome) in registry.biomes.iter() {
            let green = &table.variants()[table.variant_id(id, 0) - count];
            let red = &table.variants()[table.variant_id(id, 1) - count];
            assert!(std::ptr::eq(green.source, biome));
            assert!(std::ptr::eq(red.source, biome));
            assert_eq!(green.tint.water_color, 0x00_FF00);
            assert_eq!(red.tint.water_color, 0xFF_0000);
            assert_eq!(
                green.key,
                Identifier::new(
                    "biome_test",
                    format!("green_tint/{}/{}", biome.key.namespace, biome.key.path)
                )
            );
        }
        Ok(())
    }
}
