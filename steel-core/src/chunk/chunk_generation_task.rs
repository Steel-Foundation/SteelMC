//! `ChunkGenerationTask` handles the generation process for chunks.
use std::{
    cmp::max,
    future::Future,
    mem::{self, MaybeUninit},
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use futures::future::join_all;
use rayon::ThreadPool;
use steel_utils::{ChunkPos, locks::SyncMutex};

use crate::chunk::{
    chunk_access::ChunkStatus,
    chunk_holder::ChunkHolder,
    chunk_map::ChunkMap,
    chunk_pyramid::{GENERATION_PYRAMID, LOADING_PYRAMID},
};

/// A pre-filled 2D cache of elements, efficient for async creation.
pub struct StaticCache2D<T> {
    min_x: i32,
    min_z: i32,
    size: i32,
    /// Cache stored in row-major order (Z-then-X).
    cache: Vec<T>,
}

impl<T> StaticCache2D<T> {
    /// Creates a `StaticCache2D` by populating it via a factory.
    #[allow(clippy::missing_panics_doc)]
    pub fn create<F>(center_x: i32, center_z: i32, radius: i32, factory: F) -> Self
    where
        F: Fn(i32, i32) -> T + Send + Sync + 'static,
        T: Send + 'static,
    {
        let size = radius * 2 + 1;
        let min_x = center_x - radius;
        let min_z = center_z - radius;
        let cap = (size * size) as usize;
        let mut cache = Vec::with_capacity(cap);
        cache.resize_with(cap, MaybeUninit::uninit);

        let size_usize = size as usize;
        let factory_ref = &factory;

        cache.iter_mut().enumerate().for_each(|(index, slot)| {
            let x_offset = (index % size_usize) as i32;
            let z_offset = (index / size_usize) as i32;
            slot.write(factory_ref(min_x + x_offset, min_z + z_offset));
        });

        Self {
            min_x,
            min_z,
            size,
            // SAFETY: We know that T is Send + Sync, and that the whole cache is initialized, so we can transmute it to a Vec<T>.
            cache: unsafe { mem::transmute::<Vec<MaybeUninit<T>>, Vec<T>>(cache) },
        }
    }

    /// Gets a reference to an element by world coordinates.
    ///
    /// # Panics
    /// Panics if coordinates are out of bounds.
    #[must_use]
    pub fn get(&self, x: i32, z: i32) -> &T {
        let rel_x = x - self.min_x;
        let rel_z = z - self.min_z;

        if rel_x >= 0 && rel_x < self.size && rel_z >= 0 && rel_z < self.size {
            let index = (rel_z * self.size + rel_x) as usize;
            &self.cache[index]
        } else {
            panic!(
                "Out of bounds: ({x}, {z}) vs [({}, {}) to ({}, {})]",
                self.min_x,
                self.min_z,
                self.min_x + self.size - 1,
                self.min_z + self.size - 1
            );
        }
    }
}

/// A pinned future representing a neighbor's readiness.
pub type NeighborReady = Pin<Box<dyn Future<Output = Option<()>> + Send + Sync>>;

/// A task responsible for driving a chunk to a target status.
///
/// It works in form of layers. Imagine a pyramid, to get to the top you first need to generate the base layer. And so on.
/// This works in the same way but with chunk dependencies.
///
/// This is achieved using the Generation Pyramid and Loading Pyramid.
///
/// To make sure a chunk is only put through a stage once it uses an atomic with a CAS operation. Loading Pyramid must also be advanced with noop functions so this atomic can be driven forward.
pub struct ChunkGenerationTask {
    /// The chunk map associated with this task.
    pub chunk_map: Arc<ChunkMap>,
    /// The chunk position.
    pub pos: ChunkPos,
    /// The target generation status.
    pub target_status: ChunkStatus,
    /// The status scheduled for generation. Protected by a mutex for safe concurrent access.
    pub scheduled_status: SyncMutex<Option<ChunkStatus>>,
    /// Flag indicating if the task is cancelled.
    pub marked_for_cancel: AtomicBool,
    /// Futures for neighbors. Protected by a mutex.
    pub neighbor_ready: SyncMutex<Vec<NeighborReady>>,
    /// Cache of required chunks.
    pub cache: Arc<StaticCache2D<Arc<ChunkHolder>>>,
    /// Whether generation is required for this task.
    pub needs_generation: AtomicBool,
    /// The thread pool to use for generation.
    pub thread_pool: Arc<ThreadPool>,
}

impl ChunkGenerationTask {
    /// Creates a new generation task.
    #[must_use]
    #[allow(clippy::unwrap_used, clippy::missing_panics_doc)]
    #[inline]
    pub fn new(
        pos: ChunkPos,
        target_status: ChunkStatus,
        chunk_map: Arc<ChunkMap>,
        thread_pool: Arc<ThreadPool>,
    ) -> Self {
        let worst_case_radius = GENERATION_PYRAMID
            .get_step_to(target_status)
            .accumulated_dependencies
            .get_radius_of(ChunkStatus::Empty) as i32;

        let chunk_map_clone = chunk_map.clone();
        let cache = StaticCache2D::create(pos.0.x, pos.0.y, worst_case_radius, move |x, y| {
            chunk_map_clone
                .chunks
                .read_sync(&ChunkPos::new(x, y), |_, chunk_holder| chunk_holder.clone())
                .expect("The chunkholder should be created by distance manager before the generation task is scheduled. This occurring means there is a bug in the distance manager or you called this yourself.")
        });

        Self {
            chunk_map,
            pos,
            target_status,
            scheduled_status: SyncMutex::new(None),
            marked_for_cancel: AtomicBool::new(false),
            neighbor_ready: SyncMutex::new(Vec::new()),
            cache: Arc::new(cache),
            needs_generation: AtomicBool::new(true),
            thread_pool,
        }
    }

    /// Marks the task for cancellation.
    pub fn mark_for_cancel(&self) {
        self.marked_for_cancel.store(true, Ordering::Relaxed);
    }

    /// Schedules a chunk for a specific layer.
    ///
    /// # Panics
    /// Panics if generation is required but not expected.
    pub fn schedule_chunk_in_layer(
        &self,
        status: ChunkStatus,
        needs_generation: bool,
        chunk_holder: &Arc<ChunkHolder>,
    ) -> bool {
        let persisted_status = chunk_holder.persisted_status();

        let generate;
        if let Some(persisted_status) = persisted_status {
            generate = status > persisted_status;
        } else {
            generate = true;
        }

        let pyramid = if generate {
            &GENERATION_PYRAMID
        } else {
            &LOADING_PYRAMID
        };

        assert!(
            !generate || needs_generation,
            "Generation required but not expected for chunk load"
        );

        if let Some(future) = chunk_holder.apply_step(
            pyramid.get_step_to(status),
            &self.chunk_map,
            &self.cache,
            self.thread_pool.clone(),
        ) {
            self.neighbor_ready.lock().push(future);
        } else {
            self.mark_for_cancel();
        }

        true
    }

    /// Schedules tasks for the current layer's neighbors.
    pub fn schedule_layer(&self, status: ChunkStatus, needs_generation: bool) {
        let radius = self.get_radius_for_layer(status, needs_generation);
        // This for loop is inclusive, so if the radius is 0, we will only schedule the center chunk.
        for x in (self.pos.0.x - radius)..=(self.pos.0.x + radius) {
            for y in (self.pos.0.y - radius)..=(self.pos.0.y + radius) {
                let chunk_holder = self.cache.get(x, y);
                if self.marked_for_cancel.load(Ordering::Relaxed)
                    || !self.schedule_chunk_in_layer(status, needs_generation, chunk_holder)
                {
                    return;
                }
            }
        }
    }

    const fn get_radius_for_layer(&self, status: ChunkStatus, needs_generation: bool) -> i32 {
        let pyramid = if needs_generation {
            &GENERATION_PYRAMID
        } else {
            &LOADING_PYRAMID
        };

        pyramid
            .get_step_to(self.target_status)
            .get_accumulated_radius_of(status) as i32
    }

    /// Schedules the next layer of generation dependencies.
    ///
    /// # Panics
    /// Panics if the schedule is invalid.
    pub fn schedule_next_layer(&self) {
        let status_to_schedule;
        if self.scheduled_status.lock().is_none() {
            status_to_schedule = ChunkStatus::Empty;
        } else if !self.needs_generation.load(Ordering::Relaxed)
            && *self.scheduled_status.lock() == Some(ChunkStatus::Empty)
            && !self.can_load_without_generation()
        {
            self.needs_generation.store(true, Ordering::Relaxed);
            status_to_schedule = ChunkStatus::Empty;
        } else {
            status_to_schedule = self
                .scheduled_status
                .lock()
                .expect("Scheduled status missing")
                .next()
                .expect("Next status missing");
        }

        self.schedule_layer(
            status_to_schedule,
            self.needs_generation.load(Ordering::Relaxed),
        );
        //log::info!("Scheduled layer: {:?}", status_to_schedule);
        self.scheduled_status.lock().replace(status_to_schedule);
    }

    fn can_load_without_generation(&self) -> bool {
        if self.target_status == ChunkStatus::Empty {
            return true;
        }
        let center = self.cache.get(self.pos.0.x, self.pos.0.y);
        let highest_generated_status = center.persisted_status();

        if let Some(highest_status) = highest_generated_status {
            if highest_status < self.target_status {
                return false;
            }

            let dependencies = &LOADING_PYRAMID
                .get_step_to(self.target_status)
                .accumulated_dependencies;
            let range = dependencies.get_radius() as i32;

            for x in (self.pos.0.x - range)..=(self.pos.0.x + range) {
                for z in (self.pos.0.y - range)..=(self.pos.0.y + range) {
                    let distance = max((self.pos.0.x - x).abs(), (self.pos.0.y - z).abs()) as usize;
                    if let Some(required_status) = dependencies.get(distance) {
                        let neighbor = self.cache.get(x, z);
                        let persisted = neighbor.persisted_status();
                        if persisted < Some(required_status) {
                            return false;
                        }
                    }
                }
            }
            true
        } else {
            false
        }
    }

    /// Runs the generation task loop.
    pub async fn run(self: Arc<Self>) {
        //log::info!(
        //    "Running generation task for {:?}, target status: {:?}",
        //    self.pos,
        //    self.target_status
        //);
        loop {
            self.wait_for_scheduled_layers().await;

            if self.marked_for_cancel.load(Ordering::Relaxed)
                || *self.scheduled_status.lock() == Some(self.target_status)
            {
                let center_chunk = self.cache.get(self.pos.0.x, self.pos.0.y);
                center_chunk.cancel_generation_task();
                return;
            }

            self.schedule_next_layer();
        }
    }

    /// Waits for all scheduled neighbor tasks to complete.
    pub async fn wait_for_scheduled_layers(&self) {
        // Collect all futures first to avoid locking the mutex during await
        let futures: Vec<_> = {
            let mut lock = self.neighbor_ready.lock();
            mem::take(&mut *lock)
        };

        if futures.is_empty() {
            return;
        }

        let results = join_all(futures).await;

        for result in results {
            if result.is_none() {
                //log::error!("Neighbor ready is none for chunk {:?}", self.pos);
                self.mark_for_cancel();
                break;
            }
        }
    }
}
