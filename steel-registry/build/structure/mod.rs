//! Build-time generation for worldgen structure registries.

mod processors;
mod sets;
mod template_pools;

pub(crate) use processors::build as processors;
pub(crate) use sets::build as sets;
pub(crate) use sets::build_structures as structures;
pub(crate) use template_pools::build as template_pools;
