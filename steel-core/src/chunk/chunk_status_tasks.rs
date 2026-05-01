#![expect(
    missing_docs,
    reason = "task functions are named after their vanilla counterparts"
)]

use std::sync::Arc;

use crate::chunk::{
    chunk_generation_task::StaticCache2D, chunk_holder::ChunkHolder, chunk_pyramid::ChunkStep,
};
use crate::worldgen::{context::WorldGenContext, stages};

pub struct ChunkStatusTasks;

/// All these functions are blocking.
impl ChunkStatusTasks {
    pub fn empty(
        context: Arc<WorldGenContext>,
        step: &ChunkStep,
        cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) {
        stages::empty::generate(context, step, cache, holder);
    }

    pub fn generate_structure_starts(
        context: Arc<WorldGenContext>,
        step: &ChunkStep,
        cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) {
        stages::structures::generate_starts(context, step, cache, holder);
    }

    pub fn generate_structure_references(
        context: Arc<WorldGenContext>,
        step: &ChunkStep,
        cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) {
        stages::structures::generate_references(context, step, cache, holder);
    }

    pub fn load_structure_starts(
        context: Arc<WorldGenContext>,
        step: &ChunkStep,
        cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) {
        stages::structures::load_starts(context, step, cache, holder);
    }

    pub fn generate_biomes(
        context: Arc<WorldGenContext>,
        step: &ChunkStep,
        cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) {
        stages::biomes::generate(context, step, cache, holder);
    }

    pub fn generate_noise(
        context: Arc<WorldGenContext>,
        step: &ChunkStep,
        cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) {
        stages::noise::generate(context, step, cache, holder);
    }

    pub fn generate_surface(
        context: Arc<WorldGenContext>,
        step: &ChunkStep,
        cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) {
        stages::surface::generate(context, step, cache, holder);
    }

    pub fn generate_carvers(
        context: Arc<WorldGenContext>,
        step: &ChunkStep,
        cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) {
        stages::carvers::generate(context, step, cache, holder);
    }

    pub fn generate_features(
        context: Arc<WorldGenContext>,
        step: &ChunkStep,
        cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) {
        stages::features::generate(context, step, cache, holder);
    }

    pub fn initialize_light(
        context: Arc<WorldGenContext>,
        step: &ChunkStep,
        cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) {
        stages::light::initialize(context, step, cache, holder);
    }

    pub fn light(
        context: Arc<WorldGenContext>,
        step: &ChunkStep,
        cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) {
        stages::light::generate(context, step, cache, holder);
    }

    pub fn generate_spawn(
        context: Arc<WorldGenContext>,
        step: &ChunkStep,
        cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) {
        stages::spawn::generate(context, step, cache, holder);
    }

    pub fn full(
        context: Arc<WorldGenContext>,
        step: &ChunkStep,
        cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
        holder: Arc<ChunkHolder>,
    ) {
        stages::full::generate(context, step, cache, holder);
    }
}
