//! Brain AI.

pub mod behavior;
pub mod context;
pub mod memory;
pub mod sensor;

#[cfg(test)]
mod test_support;

pub use context::BrainContext;

/// Initialize the brain registries.
pub fn init_brain_registries() {
    memory::init_memory_module_types();
}
