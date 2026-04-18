//! This module contains the `ChunkPyramid`, which is used to check chunk dependencies.
//! All structures are const-compatible and computed at compile time.

use std::sync::Arc;

use crate::chunk::{
    chunk_access::ChunkStatus, chunk_generation_task::StaticCache2D, chunk_holder::ChunkHolder,
    chunk_status_tasks::ChunkStatusTasks, world_gen_context::WorldGenContext,
};

/// Number of `ChunkStatus` variants.
const STATUS_COUNT: usize = 12;
/// Maximum dependency radius supported.
const MAX_RADIUS: usize = 16;

/// A collection of chunk dependencies (const-compatible).
#[derive(Debug, Clone, Copy)]
pub struct ChunkDependencies {
    dependency_by_radius: [Option<ChunkStatus>; MAX_RADIUS],
    len: usize,
    radius_by_dependency: [usize; STATUS_COUNT],
}

impl ChunkDependencies {
    /// Empty dependencies constant.
    pub const EMPTY: Self = Self {
        dependency_by_radius: [None; MAX_RADIUS],
        len: 0,
        radius_by_dependency: [0; STATUS_COUNT],
    };

    /// Creates dependencies from requirements and optional parent status.
    #[must_use]
    const fn from_requirements(
        reqs: &[(ChunkStatus, usize)],
        parent_status: Option<ChunkStatus>,
    ) -> Self {
        let mut dependency_by_radius = [None; MAX_RADIUS];
        let mut len = 0;

        // If we have a parent, start with parent at radius 0
        if let Some(parent) = parent_status {
            dependency_by_radius[0] = Some(parent);
            len = 1;
        }

        // Process requirements
        let mut i = 0;
        while i < reqs.len() {
            let (status, radius) = reqs[i];
            let new_len = radius + 1;

            // Extend if needed, filling with this status
            if new_len > len {
                let mut j = len;
                while j < new_len {
                    dependency_by_radius[j] = Some(status);
                    j += 1;
                }
                len = new_len;
            }

            // Update existing entries if this status is higher
            let limit = const_min(len, new_len);
            let mut j = 0;
            while j < limit {
                if let Some(existing) = dependency_by_radius[j]
                    && status.get_index() > existing.get_index()
                {
                    dependency_by_radius[j] = Some(status);
                }
                j += 1;
            }

            i += 1;
        }

        // Build radius_by_dependency
        let radius_by_dependency = Self::build_radius_lookup(&dependency_by_radius, len);

        Self {
            dependency_by_radius,
            len,
            radius_by_dependency,
        }
    }

    /// Builds the radius lookup table from dependency array.
    const fn build_radius_lookup(
        deps: &[Option<ChunkStatus>; MAX_RADIUS],
        len: usize,
    ) -> [usize; STATUS_COUNT] {
        let mut radius_by_dependency = [0usize; STATUS_COUNT];
        let mut radius = 0;
        while radius < len {
            if let Some(dep) = deps[radius] {
                let index = dep.get_index();
                let mut j = 0;
                while j <= index && j < STATUS_COUNT {
                    radius_by_dependency[j] = radius;
                    j += 1;
                }
            }
            radius += 1;
        }
        radius_by_dependency
    }

    /// Computes accumulated dependencies by merging with parent's accumulated dependencies.
    const fn accumulate(&self, parent_accumulated: &Self, parent_status: ChunkStatus) -> Self {
        // Find the last radius where we reference the parent status or higher
        let mut radius_of_parent = 0;
        let mut i = 0;
        while i < self.len {
            if let Some(s) = self.dependency_by_radius[i]
                && s.get_index() >= parent_status.get_index()
            {
                radius_of_parent = i;
            }
            i += 1;
        }

        let parent_len = parent_accumulated.len;
        let new_len = const_max(radius_of_parent + parent_len, self.len);
        let capped_len = const_min(new_len, MAX_RADIUS);

        let mut accumulated = [None; MAX_RADIUS];

        let mut dist = 0;
        while dist < capped_len {
            let dist_in_parent = dist.saturating_sub(radius_of_parent);

            let parent_dep = if dist_in_parent < parent_accumulated.len {
                parent_accumulated.dependency_by_radius[dist_in_parent]
            } else {
                None
            };

            let direct_dep = if dist < self.len {
                self.dependency_by_radius[dist]
            } else {
                None
            };

            accumulated[dist] = const_max_status(direct_dep, parent_dep);
            dist += 1;
        }

        let radius_by_dependency = Self::build_radius_lookup(&accumulated, capped_len);

        Self {
            dependency_by_radius: accumulated,
            len: capped_len,
            radius_by_dependency,
        }
    }

    /// Gets the radius of the dependencies for the given status.
    ///
    /// # Panics
    /// Panics if the status index is out of bounds.
    #[must_use]
    pub const fn get_radius_of(&self, status: ChunkStatus) -> usize {
        self.radius_by_dependency[status.get_index()]
    }

    /// Gets the radius of the dependencies.
    #[must_use]
    pub const fn get_radius(&self) -> usize {
        self.len.saturating_sub(1)
    }

    /// Gets the dependency status at the given distance.
    #[must_use]
    pub const fn get(&self, distance: usize) -> Option<ChunkStatus> {
        if distance < self.len {
            self.dependency_by_radius[distance]
        } else {
            None
        }
    }
}

// ============================================================================
// Const helper functions
// ============================================================================

const fn const_max(a: usize, b: usize) -> usize {
    if a > b { a } else { b }
}

const fn const_min(a: usize, b: usize) -> usize {
    if a < b { a } else { b }
}

const fn const_max_status(a: Option<ChunkStatus>, b: Option<ChunkStatus>) -> Option<ChunkStatus> {
    match (a, b) {
        (Some(sa), Some(sb)) => {
            if sa.get_index() > sb.get_index() {
                Some(sa)
            } else {
                Some(sb)
            }
        }
        (Some(s), None) | (None, Some(s)) => Some(s),
        (None, None) => None,
    }
}

// ============================================================================
// ChunkStep
// ============================================================================

/// A task that generates a chunk.
pub type ChunkStatusTask =
    fn(Arc<WorldGenContext>, &ChunkStep, &Arc<StaticCache2D<Arc<ChunkHolder>>>, Arc<ChunkHolder>);

/// A chunk step (const-compatible).
#[derive(Clone, Copy)]
pub struct ChunkStep {
    /// The target status of the step.
    pub target_status: ChunkStatus,
    /// The direct dependencies of the step.
    pub direct_dependencies: ChunkDependencies,
    /// The accumulated dependencies of the step.
    pub accumulated_dependencies: ChunkDependencies,
    /// The block state write radius of the step.
    pub block_state_write_radius: i32,
    /// The task of the step.
    pub task: ChunkStatusTask,
}

impl ChunkStep {
    /// A placeholder step used for array initialization.
    const PLACEHOLDER: Self = Self {
        target_status: ChunkStatus::Empty,
        direct_dependencies: ChunkDependencies::EMPTY,
        accumulated_dependencies: ChunkDependencies::EMPTY,
        block_state_write_radius: -1,
        task: noop_task,
    };

    /// Gets the accumulated radius of the dependencies for the given status.
    #[must_use]
    pub const fn get_accumulated_radius_of(&self, status: ChunkStatus) -> usize {
        if status.get_index() == self.target_status.get_index() {
            0
        } else {
            self.accumulated_dependencies.get_radius_of(status)
        }
    }
}

fn noop_task(
    _context: Arc<WorldGenContext>,
    _step: &ChunkStep,
    _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
    _holder: Arc<ChunkHolder>,
) {
}

// ============================================================================
// ChunkPyramid and const builder
// ============================================================================

/// Represents the hierarchy and dependencies for chunk generation or loading.
pub struct ChunkPyramid {
    steps: [ChunkStep; STATUS_COUNT],
}

impl ChunkPyramid {
    /// Gets the step for the given status.
    #[must_use]
    pub const fn get_step_to(&self, status: ChunkStatus) -> &ChunkStep {
        &self.steps[status.get_index()]
    }
}

/// Const-time pyramid builder.
struct ConstPyramidBuilder {
    steps: [ChunkStep; STATUS_COUNT],
    count: usize,
}

impl ConstPyramidBuilder {
    const fn new() -> Self {
        Self {
            steps: [ChunkStep::PLACEHOLDER; STATUS_COUNT],
            count: 0,
        }
    }

    const fn step(
        mut self,
        status: ChunkStatus,
        requirements: &[(ChunkStatus, usize)],
        block_state_write_radius: i32,
        task: ChunkStatusTask,
    ) -> Self {
        // Get parent info if we have previous steps
        let (parent_status, parent_accumulated) = if self.count > 0 {
            let parent = &self.steps[self.count - 1];
            (
                Some(parent.target_status),
                Some(parent.accumulated_dependencies),
            )
        } else {
            (None, None)
        };

        // Compute direct dependencies
        let direct = ChunkDependencies::from_requirements(requirements, parent_status);

        // Compute accumulated dependencies
        let accumulated = match (parent_status, parent_accumulated) {
            (Some(ps), Some(pa)) => direct.accumulate(&pa, ps),
            _ => direct,
        };

        self.steps[self.count] = ChunkStep {
            target_status: status,
            direct_dependencies: direct,
            accumulated_dependencies: accumulated,
            block_state_write_radius,
            task,
        };
        self.count += 1;
        self
    }

    const fn build(self) -> ChunkPyramid {
        ChunkPyramid { steps: self.steps }
    }
}

// ============================================================================
// Macro for ergonomic pyramid definition
// ============================================================================

/// Macro for defining chunk pyramids with nice syntax.
///
/// # Example
/// ```ignore
/// define_pyramid! {
///     pub static MY_PYRAMID = {
///         Empty => { task: my_task },
///         StructureStarts => {
///             requirements: [(StructureStarts, 8)],
///             task: other_task,
///         },
///     };
/// }
/// ```
macro_rules! define_pyramid {
    (
        $vis:vis const $name:ident = {
            $($status:ident => {
                $(requirements: [$( ($req_status:ident, $req_radius:expr) ),* $(,)?] ,)?
                $(block_state_write_radius: $bswr:expr ,)?
                task: $task:expr $(,)?
            }),* $(,)?
        };
    ) => {
        #[expect(missing_docs, reason = "generated pyramid constant; name is self-documenting")]
        $vis const $name: &'static ChunkPyramid = &{
            ConstPyramidBuilder::new()
            $(
                .step(
                    ChunkStatus::$status,
                    &[ $( $( (ChunkStatus::$req_status, $req_radius) ),* )? ],
                    define_pyramid!(@bswr $($bswr)?),
                    $task,
                )
            )*
            .build()
        };
    };

    // Default block_state_write_radius
    (@bswr) => { -1 };
    (@bswr $bswr:expr) => { $bswr };
}

// ============================================================================
// Pyramid definitions
// ============================================================================

define_pyramid! {
    pub const GENERATION_PYRAMID = {
        Empty => {
            task: ChunkStatusTasks::empty,
        },
        StructureStarts => {
            task: ChunkStatusTasks::generate_structure_starts,
        },
        StructureReferences => {
            requirements: [(StructureStarts, 8)],
            task: ChunkStatusTasks::generate_structure_references,
        },
        Biomes => {
            requirements: [(StructureStarts, 8)],
            task: ChunkStatusTasks::generate_biomes,
        },
        Noise => {
            requirements: [(StructureStarts, 8), (Biomes, 1)],
            block_state_write_radius: 0,
            task: ChunkStatusTasks::generate_noise,
        },
        Surface => {
            requirements: [(StructureStarts, 8), (Biomes, 1)],
            block_state_write_radius: 0,
            task: ChunkStatusTasks::generate_surface,
        },
        Carvers => {
            requirements: [(StructureStarts, 8)],
            block_state_write_radius: 0,
            task: ChunkStatusTasks::generate_carvers,
        },
        Features => {
            requirements: [(StructureStarts, 8), (Carvers, 1)],
            block_state_write_radius: 1,
            task: ChunkStatusTasks::generate_features,
        },
        InitializeLight => {
            task: ChunkStatusTasks::initialize_light,
        },
        Light => {
            requirements: [(InitializeLight, 1)],
            task: ChunkStatusTasks::light,
        },
        Spawn => {
            requirements: [(Biomes, 1)],
            task: ChunkStatusTasks::generate_spawn,
        },
        Full => {
            task: ChunkStatusTasks::full,
        },
    };
}

define_pyramid! {
    pub const LOADING_PYRAMID = {
        Empty => {
            task: noop_task,
        },
        StructureStarts => {
            task: ChunkStatusTasks::load_structure_starts,
        },
        StructureReferences => {
            task: noop_task,
        },
        Biomes => {
            task: noop_task,
        },
        Noise => {
            task: noop_task,
        },
        Surface => {
            task: noop_task,
        },
        Carvers => {
            task: noop_task,
        },
        Features => {
            task: noop_task,
        },
        InitializeLight => {
            task: ChunkStatusTasks::initialize_light,
        },
        Light => {
            requirements: [(InitializeLight, 1)],
            task: ChunkStatusTasks::light,
        },
        Spawn => {
            task: noop_task,
        },
        Full => {
            task: ChunkStatusTasks::full,
        },
    };
}
