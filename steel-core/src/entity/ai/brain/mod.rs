//! Brain AI.

pub mod memory;

/// Initialize the brain registries.
pub fn init_brain_registries() {
    memory::init_memory_module_types();
}
