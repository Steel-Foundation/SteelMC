//! Tick-sliced completion of exploration-map container loot.

use std::{mem, sync::Arc};

use steel_registry::{
    REGISTRY, RegistryExt, RegistryReference, TaggedRegistryExt,
    data_components::{
        MapDecorationEntry, MapDecorations, MapId, MapItemColor,
        vanilla_components::{MAP_COLOR, MAP_DECORATIONS, MAP_ID},
    },
    item_stack::ItemStack,
    map_decoration_type::MapDecorationTypeRef,
};
use steel_utils::{BlockPos, Downcast as _};

use crate::{
    inventory::lock::ContainerRef,
    map::{
        NewMapData,
        biome_preview::{BiomePreview, BiomePreviewPoll},
    },
    server::jobs::{JobPoll, ServerJob, ServerJobContext},
    world::World,
    worldgen::{
        generator::ChunkGenerator as _,
        structure::locate::{StructureLocatePoll, StructureLocator},
    },
};

use super::{PendingExplorationMap, RandomizableContainer};

enum ExplorationPhase {
    Start,
    Locating(StructureLocator),
    Rendering {
        target: BlockPos,
        preview: BiomePreview,
    },
    Finished,
}

struct ResolvedExplorationMap {
    marker: i32,
    target_and_colors: Option<(BlockPos, Vec<u8>)>,
}

enum PublishResult {
    Stale,
    Published {
        removed: bool,
        dropped: Vec<ItemStack>,
    },
}

struct PreparedPublication {
    decorations: Vec<MapDecorationTypeRef>,
    map_ids: Vec<MapId>,
}

pub(super) struct ExplorationMapJob {
    world: Arc<World>,
    owner_pos: BlockPos,
    container: ContainerRef,
    token: u64,
    maps: Vec<PendingExplorationMap>,
    current: usize,
    phase: ExplorationPhase,
    resolved: Vec<ResolvedExplorationMap>,
}

impl ExplorationMapJob {
    pub(super) const fn new(
        world: Arc<World>,
        owner_pos: BlockPos,
        container: ContainerRef,
        token: u64,
        maps: Vec<PendingExplorationMap>,
    ) -> Self {
        Self {
            world,
            owner_pos,
            container,
            token,
            maps,
            current: 0,
            phase: ExplorationPhase::Start,
            resolved: Vec::new(),
        }
    }

    fn is_current(&self) -> bool {
        self.container.with_locked_mut(|storage| {
            storage
                .downcast_mut::<RandomizableContainer>()
                .is_some_and(|storage| storage.is_running(self.token))
        })
    }

    fn begin_current_search(&mut self) -> Result<(), String> {
        let Some(map) = self.maps.get(self.current) else {
            self.phase = ExplorationPhase::Finished;
            return Ok(());
        };
        let structures = REGISTRY
            .structures
            .get_tag(&map.request.destination)
            .ok_or_else(|| format!("unknown structure tag {}", map.request.destination))?;
        let structure_keys = structures
            .iter()
            .map(|structure| structure.key.clone())
            .collect::<Vec<_>>();
        let Some(generator) = self
            .world
            .chunk_map
            .world_gen_context
            .generator
            .structure_generator()
        else {
            self.resolve_current(None);
            return Ok(());
        };
        let Some(plan) = generator.locate_plan_for_structures(&structure_keys) else {
            self.resolve_current(None);
            return Ok(());
        };
        self.phase = ExplorationPhase::Locating(StructureLocator::new(
            Arc::clone(&self.world),
            plan,
            self.owner_pos,
            map.request.search_radius,
            map.request.skip_existing_chunks,
        ));
        Ok(())
    }

    fn resolve_current(&mut self, target_and_colors: Option<(BlockPos, Vec<u8>)>) {
        if let Some(map) = self.maps.get(self.current) {
            self.resolved.push(ResolvedExplorationMap {
                marker: map.marker,
                target_and_colors,
            });
        }
        self.current += 1;
        self.phase = if self.current == self.maps.len() {
            ExplorationPhase::Finished
        } else {
            ExplorationPhase::Start
        };
    }

    fn validate_pending(&self) -> bool {
        self.container.with_locked_mut(|storage| {
            storage
                .downcast_mut::<RandomizableContainer>()
                .is_some_and(|storage| storage.pending_markers_are_valid(self.token))
        })
    }

    fn prepare_publication(&self) -> Result<PreparedPublication, String> {
        let mut decorations = Vec::with_capacity(self.maps.len());
        let mut new_maps = Vec::new();
        for (map, resolved) in self.maps.iter().zip(&self.resolved) {
            if map.marker != resolved.marker {
                return Err("exploration-map marker order changed while resolving loot".to_owned());
            }
            let decoration = REGISTRY
                .map_decoration_types
                .by_key(&map.request.decoration)
                .ok_or_else(|| format!("unknown map decoration type {}", map.request.decoration))?;
            decorations.push(decoration);
            if let Some((target, colors)) = &resolved.target_and_colors {
                let scale = i8::try_from(map.request.zoom)
                    .map_err(|_| format!("invalid exploration-map zoom {}", map.request.zoom))?;
                new_maps.push(NewMapData {
                    origin: *target,
                    scale,
                    tracking_position: true,
                    unlimited_tracking: true,
                    world: self.world.key.clone(),
                    dimension_type: self.world.dimension_type.key.clone(),
                    colors: colors.clone(),
                });
            }
        }

        let map_ids = self
            .world
            .map_data()
            .create_maps(new_maps)
            .map_err(|error| format!("could not allocate exploration-map data: {error}"))?;
        let expected_ids = self
            .resolved
            .iter()
            .filter(|resolved| resolved.target_and_colors.is_some())
            .count();
        if map_ids.len() != expected_ids {
            return Err("map-data store returned an incomplete allocation batch".to_owned());
        }
        Ok(PreparedPublication {
            decorations,
            map_ids,
        })
    }

    fn publish(&mut self) -> Result<PublishResult, String> {
        if !self.is_current() {
            return Ok(PublishResult::Stale);
        }
        if !self.validate_pending() {
            return Err(
                "pending exploration-map markers no longer match container loot".to_owned(),
            );
        }

        let publication = self.prepare_publication()?;

        let mut next_id = 0;
        let result = self.container.with_locked_mut(|storage| {
            let Some(storage) = storage.downcast_mut::<RandomizableContainer>() else {
                return PublishResult::Stale;
            };
            if !storage.pending_markers_are_valid(self.token) {
                return PublishResult::Stale;
            }
            let Some(mut pending) = storage.pending_loot.take() else {
                return PublishResult::Stale;
            };

            for ((map, resolved), decoration) in pending
                .maps
                .iter()
                .zip(&self.resolved)
                .zip(publication.decorations.iter().copied())
            {
                let Some(slot) = pending
                    .items
                    .iter()
                    .position(|item| item.get(MAP_ID).is_some_and(|id| id.id() == map.marker))
                else {
                    storage.pending_loot = Some(pending);
                    return PublishResult::Stale;
                };
                let placeholder = &pending.items[slot];
                let Some((target, _)) = &resolved.target_and_colors else {
                    pending.items[slot] =
                        RandomizableContainer::restore_fallback_item(placeholder, map);
                    continue;
                };
                let Some(map_id) = publication.map_ids.get(next_id).copied() else {
                    storage.pending_loot = Some(pending);
                    return PublishResult::Stale;
                };
                next_id += 1;
                let item = &mut pending.items[slot];
                item.set(MAP_ID, map_id);
                let entry = MapDecorationEntry::new(
                    RegistryReference::new(decoration),
                    f64::from(target.x()),
                    f64::from(target.z()),
                    180.0,
                );
                let current = item.get_or_default(MAP_DECORATIONS, MapDecorations::EMPTY);
                item.set(
                    MAP_DECORATIONS,
                    current.with_decoration("+".to_owned(), entry),
                );
                if decoration.has_map_color() {
                    item.set(MAP_COLOR, MapItemColor::new(decoration.map_color));
                }
            }

            let removed = storage.removed;
            if let Err(items) = storage.base.replace_items(pending.items) {
                pending.items = items;
                storage.pending_loot = Some(pending);
                storage.reset_running(self.token);
                return PublishResult::Stale;
            }
            storage.loot_table = None;
            storage.running_token = None;
            let dropped = if removed {
                storage.base.take_items()
            } else {
                Vec::new()
            };
            PublishResult::Published { removed, dropped }
        });
        Ok(result)
    }

    fn finish_publish(&mut self) -> JobPoll {
        match self.publish() {
            Ok(PublishResult::Stale) => JobPoll::Finished,
            Ok(PublishResult::Published { removed, dropped }) => {
                if removed {
                    self.drop_items(dropped);
                } else {
                    self.container.notify_owner_changed();
                }
                JobPoll::Finished
            }
            Err(error) => self.fail(error),
        }
    }

    fn fail(&mut self, error: String) -> JobPoll {
        log::error!("Failed to finish deferred exploration-map loot: {error}");
        let dropped = self.container.with_locked_mut(|storage| {
            let Some(storage) = storage.downcast_mut::<RandomizableContainer>() else {
                return Vec::new();
            };
            if storage.removed {
                storage.finish_removed_as_fallback(self.token)
            } else {
                storage.reset_running(self.token);
                Vec::new()
            }
        });
        self.drop_items(dropped);
        JobPoll::Finished
    }

    fn drop_items(&self, items: Vec<ItemStack>) {
        for item in items {
            self.world.drop_item_stack(self.owner_pos, item);
        }
    }

    fn cancel_locator(&mut self) {
        if let ExplorationPhase::Locating(locator) = &mut self.phase {
            locator.cancel();
        }
    }
}

impl ServerJob for ExplorationMapJob {
    fn poll(&mut self, _context: &mut ServerJobContext) -> JobPoll {
        if !self.is_current() {
            self.cancel_locator();
            return JobPoll::Finished;
        }

        loop {
            let phase = mem::replace(&mut self.phase, ExplorationPhase::Finished);
            match phase {
                ExplorationPhase::Start => {
                    if let Err(error) = self.begin_current_search() {
                        return self.fail(error);
                    }
                }
                ExplorationPhase::Locating(mut locator) => match locator.poll() {
                    StructureLocatePoll::Pending => {
                        self.phase = ExplorationPhase::Locating(locator);
                        return JobPoll::Pending;
                    }
                    StructureLocatePoll::Cancelled => {
                        return self.fail("structure location was cancelled".to_owned());
                    }
                    StructureLocatePoll::Ready(None) => self.resolve_current(None),
                    StructureLocatePoll::Ready(Some(located)) => {
                        let Some(map) = self.maps.get(self.current) else {
                            return self.fail(
                                "exploration-map request disappeared during location".to_owned(),
                            );
                        };
                        self.phase = ExplorationPhase::Rendering {
                            target: located.pos,
                            preview: BiomePreview::new(located.pos, map.request.zoom as i8),
                        };
                    }
                },
                ExplorationPhase::Rendering {
                    target,
                    mut preview,
                } => match preview.poll(&self.world) {
                    BiomePreviewPoll::Pending => {
                        self.phase = ExplorationPhase::Rendering { target, preview };
                        return JobPoll::Pending;
                    }
                    BiomePreviewPoll::Ready(colors) => {
                        self.resolve_current(Some((target, colors)));
                    }
                    BiomePreviewPoll::MissingBiome(pos) => {
                        return self.fail(format!(
                            "biome data was unavailable at {pos:?} while rendering a map"
                        ));
                    }
                },
                ExplorationPhase::Finished => return self.finish_publish(),
            }
        }
    }

    fn cancel(&mut self) {
        self.cancel_locator();
        let dropped = self.container.with_locked_mut(|storage| {
            let Some(storage) = storage.downcast_mut::<RandomizableContainer>() else {
                return Vec::new();
            };
            if storage.removed {
                storage.finish_removed_as_fallback(self.token)
            } else {
                storage.reset_running(self.token);
                Vec::new()
            }
        });
        self.drop_items(dropped);
    }
}
