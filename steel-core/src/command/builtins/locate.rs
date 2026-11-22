//! Structure, biome, and POI location command.

use std::{io, sync::Arc, time::Instant};

use steel_registry::poi::PoiTypeRef;
use steel_utils::{BlockPos, Identifier, translations};
use text_components::{
    Modifier, TextComponent,
    format::Color,
    interactivity::{ClickEvent, HoverEvent},
    translation::Translation,
};
use tokio::{sync::oneshot, task::JoinHandle};

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        BiomeOrTag, CommandResultSuspension, CommandResultSuspensionPoll, CommandSource, PoiOrTag,
        SteelArgumentType, SteelCommandContext, SteelCommandRuntime, StructureOrTagKey, argument,
        literal,
    },
    registration::CommandRegistration,
};
use crate::{
    chunk::{
        chunk_request::{ChunkRequest, ChunkRequestHandle, ChunkRequestState, ChunkTicketKind},
        status::ChunkStatus,
    },
    poi::OccupationStatus,
    world::World,
    worldgen::{
        generator::ChunkGenerator,
        structure::{StructureLocateCandidate, StructureLocatePlan, squared_distance},
    },
};

const MAX_STRUCTURE_SEARCH_RADIUS: i32 = 100;
const MAX_BIOME_SEARCH_RADIUS: i32 = 6400;
const BIOME_SAMPLE_RESOLUTION_HORIZONTAL: i32 = 32;
const BIOME_SAMPLE_RESOLUTION_VERTICAL: i32 = 64;
const POI_SEARCH_RADIUS: i32 = 256;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("locate"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("locate")
        .then(
            literal("structure").then(
                argument("structure", SteelArgumentType::structure_or_tag_key())
                    .executes_suspended(start_structure_search),
            ),
        )
        .then(
            literal("biome")
                .then(argument("biome", SteelArgumentType::biome_or_tag()).executes(locate_biome)),
        )
        .then(literal("poi").then(
            argument("poi", SteelArgumentType::poi_or_tag()).executes_suspended(start_poi_search),
        ))
}

fn locate_biome(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let query = context.biome_or_tag("biome")?;
    let world = context.source().world();
    let origin = BlockPos::from(context.source().position());
    let started_at = Instant::now();

    let nearest = world
        .chunk_map
        .world_gen_context
        .generator
        .find_closest_biome_3d(
            origin,
            MAX_BIOME_SEARCH_RADIUS,
            BIOME_SAMPLE_RESOLUTION_HORIZONTAL,
            BIOME_SAMPLE_RESOLUTION_VERTICAL,
            &|biome| query.matches(biome),
        );
    let Some((pos, biome)) = nearest else {
        return Err(biome_not_found(query));
    };

    let distance = distance_3d(origin, pos);
    let found_name = query.found_name(biome);
    context.source().send_success(
        &locate_success_component(
            &translations::COMMANDS_LOCATE_BIOME_SUCCESS,
            found_name.clone(),
            pos,
            distance,
            true,
        ),
        false,
    );
    tracing::info!(
        "Locating element {} took {} ms",
        found_name,
        started_at.elapsed().as_millis()
    );
    Ok(distance)
}

fn start_poi_search(
    context: &SteelCommandContext<CommandSource>,
) -> Result<LocatePoiSearch, CommandSyntaxError> {
    let query = context.poi_or_tag("poi")?.clone();
    let source = context.source().clone();
    let origin = BlockPos::from(context.source().position());
    let task_query = query.clone();
    let task_world = Arc::clone(source.world());
    let (sender, receiver) = oneshot::channel();
    // POI columns may need disk reads, so search off the tick thread and report
    // back through the command suspension queue.
    let task = tokio::spawn(async move {
        let result = task_world
            .find_closest_poi(
                |poi_type| task_query.matches(poi_type),
                origin,
                POI_SEARCH_RADIUS,
                OccupationStatus::Any,
            )
            .await;
        let _ = sender.send(result);
    });

    Ok(LocatePoiSearch {
        source,
        query,
        origin,
        receiver,
        task: Some(task),
        started_at: Instant::now(),
    })
}

struct LocatePoiSearch {
    source: CommandSource,
    query: PoiOrTag,
    origin: BlockPos,
    receiver: oneshot::Receiver<io::Result<Option<(BlockPos, PoiTypeRef)>>>,
    task: Option<JoinHandle<()>>,
    started_at: Instant,
}

impl CommandResultSuspension for LocatePoiSearch {
    fn poll(&mut self) -> CommandResultSuspensionPoll {
        let result = match self.receiver.try_recv() {
            Ok(result) => result,
            Err(oneshot::error::TryRecvError::Empty) => {
                return CommandResultSuspensionPoll::Pending;
            }
            Err(oneshot::error::TryRecvError::Closed) => {
                self.task = None;
                return CommandResultSuspensionPoll::Ready(Err(CommandSyntaxError::dynamic(
                    "Point of interest search ended without a result",
                )));
            }
        };
        self.task = None;

        let found = match result {
            Ok(found) => found,
            Err(error) => {
                tracing::error!("Failed to search for points of interest: {error}");
                return CommandResultSuspensionPoll::Ready(Err(CommandSyntaxError::dynamic(
                    "Failed to search for points of interest",
                )));
            }
        };
        let Some((pos, poi_type)) = found else {
            return CommandResultSuspensionPoll::Ready(Err(poi_not_found(&self.query)));
        };

        let distance = horizontal_distance(self.origin, pos);
        let found_name = self.query.found_name(poi_type);
        self.source.send_success(
            &locate_success_component(
                &translations::COMMANDS_LOCATE_POI_SUCCESS,
                found_name.clone(),
                pos,
                distance,
                false,
            ),
            false,
        );
        tracing::info!(
            "Locating element {} took {} ms",
            found_name,
            self.started_at.elapsed().as_millis()
        );
        CommandResultSuspensionPoll::Ready(Ok(distance))
    }

    fn cancel(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

fn start_structure_search(
    context: &SteelCommandContext<CommandSource>,
) -> Result<LocateStructureSearch, CommandSyntaxError> {
    let query = context.structure_or_tag_key("structure")?;
    let Some(structures) = query.resolve() else {
        return Err(invalid_structure(query));
    };
    if structures.is_empty() {
        return Err(structure_not_found(query));
    }

    let world = context.source().world();
    let Some(structure_generator) = world
        .chunk_map
        .world_gen_context
        .generator
        .structure_generator()
    else {
        return Err(structure_not_found(query));
    };
    let structure_keys = structures
        .iter()
        .map(|structure| structure.key.clone())
        .collect::<Vec<_>>();
    let Some(plan) = structure_generator.locate_plan_for_structures(&structure_keys) else {
        return Err(structure_not_found(query));
    };
    if plan.is_empty() {
        return Err(structure_not_found(query));
    }

    Ok(LocateStructureSearch {
        source: context.source().clone(),
        world: Arc::clone(world),
        query: query.clone(),
        plan,
        origin: BlockPos::from(context.source().position()),
        phase: LocatePhase::Start,
        pending: None,
        candidates: Vec::new(),
        best: None,
        random_radius: 0,
        started_at: Instant::now(),
    })
}

enum LocatePhase {
    Start,
    WaitingRings,
    RandomSpread,
    WaitingRandomSpread,
}

struct LocatedStructure {
    candidate: StructureLocateCandidate,
    found_structure: Identifier,
    distance_sqr: i64,
}

struct LocateStructureSearch {
    source: CommandSource,
    world: Arc<World>,
    query: StructureOrTagKey,
    plan: StructureLocatePlan,
    origin: BlockPos,
    phase: LocatePhase,
    pending: Option<ChunkRequestHandle>,
    candidates: Vec<StructureLocateCandidate>,
    best: Option<LocatedStructure>,
    random_radius: i32,
    started_at: Instant,
}

impl CommandResultSuspension for LocateStructureSearch {
    fn poll(&mut self) -> CommandResultSuspensionPoll {
        loop {
            match self.phase {
                LocatePhase::Start => {
                    self.candidates = self.plan.ring_candidates(self.origin);
                    if self.candidates.is_empty() {
                        self.phase = LocatePhase::RandomSpread;
                        continue;
                    }
                    self.pending = Some(self.request_current_candidates());
                    self.phase = LocatePhase::WaitingRings;
                    return CommandResultSuspensionPoll::Pending;
                }
                LocatePhase::WaitingRings => match self.poll_pending_request() {
                    PendingRequest::Pending => return CommandResultSuspensionPoll::Pending,
                    PendingRequest::Cancelled => return Self::cancelled_result(),
                    PendingRequest::Ready => {
                        self.best = self.first_valid_candidate();
                        self.clear_request();

                        if self.best.is_some() && !self.plan.has_random_spread() {
                            return self.success_result();
                        }

                        self.phase = LocatePhase::RandomSpread;
                    }
                },
                LocatePhase::RandomSpread => {
                    if self.random_radius > MAX_STRUCTURE_SEARCH_RADIUS {
                        return self.finished_result();
                    }

                    self.candidates = self
                        .plan
                        .random_spread_candidates_at_radius(self.origin, self.random_radius);
                    self.random_radius += 1;

                    if self.candidates.is_empty() {
                        continue;
                    }

                    self.pending = Some(self.request_current_candidates());
                    self.phase = LocatePhase::WaitingRandomSpread;
                    return CommandResultSuspensionPoll::Pending;
                }
                LocatePhase::WaitingRandomSpread => match self.poll_pending_request() {
                    PendingRequest::Pending => return CommandResultSuspensionPoll::Pending,
                    PendingRequest::Cancelled => return Self::cancelled_result(),
                    PendingRequest::Ready => {
                        if self.update_best_after_random_radius() {
                            return self.success_result();
                        }

                        self.clear_request();
                        self.phase = LocatePhase::RandomSpread;
                    }
                },
            }
        }
    }

    fn cancel(&mut self) {
        if let Some(pending) = &mut self.pending {
            pending.cancel();
        }
    }
}

impl LocateStructureSearch {
    fn request_current_candidates(&self) -> ChunkRequestHandle {
        let positions = self
            .candidates
            .iter()
            .map(|candidate| candidate.chunk_pos)
            .collect();
        self.world.chunk_map.request_chunks(ChunkRequest {
            status: ChunkStatus::StructureStarts,
            positions,
            ticket_kind: ChunkTicketKind::StructureLocate,
        })
    }

    fn poll_pending_request(&self) -> PendingRequest {
        let Some(pending) = &self.pending else {
            return PendingRequest::Cancelled;
        };

        match pending.poll() {
            ChunkRequestState::Pending { .. } => PendingRequest::Pending,
            ChunkRequestState::Ready => PendingRequest::Ready,
            ChunkRequestState::Cancelled => PendingRequest::Cancelled,
        }
    }

    fn clear_request(&mut self) {
        self.pending = None;
        self.candidates.clear();
    }

    fn first_valid_candidate(&self) -> Option<LocatedStructure> {
        self.candidates.iter().copied().find_map(|candidate| {
            self.generated_structure_at_candidate(candidate)
                .map(|found_structure| LocatedStructure {
                    candidate,
                    found_structure,
                    distance_sqr: squared_distance(candidate.locate_pos, self.origin),
                })
        })
    }

    fn update_best_after_random_radius(&mut self) -> bool {
        let mut best = self.best.take();
        let mut current_scan = None;
        let mut found_current_scan = false;
        let mut found_in_this_radius = false;

        for candidate in &self.candidates {
            if current_scan != Some(candidate.scan_id()) {
                current_scan = Some(candidate.scan_id());
                found_current_scan = false;
            }

            if found_current_scan {
                continue;
            }

            let Some(found_structure) = self.generated_structure_at_candidate(*candidate) else {
                continue;
            };
            found_current_scan = true;
            found_in_this_radius = true;
            let located = LocatedStructure {
                candidate: *candidate,
                found_structure,
                distance_sqr: squared_distance(candidate.locate_pos, self.origin),
            };
            if best
                .as_ref()
                .is_none_or(|current| located.distance_sqr < current.distance_sqr)
            {
                best = Some(located);
            }
        }

        self.best = best;
        found_in_this_radius
    }

    fn generated_structure_at_candidate(
        &self,
        candidate: StructureLocateCandidate,
    ) -> Option<Identifier> {
        let holder = self
            .world
            .chunk_map
            .chunks
            .read_sync(&candidate.chunk_pos, |_, holder| Arc::clone(holder))?;
        let chunk = holder.try_chunk(ChunkStatus::StructureStarts)?;
        let starts = chunk.structure_starts();
        let structures = self.plan.structures_for_candidate(candidate)?;
        structures.iter().find_map(|structure| {
            starts
                .get(structure)
                .is_some_and(|start| !start.pieces.is_empty())
                .then(|| structure.clone())
        })
    }

    fn finished_result(&self) -> CommandResultSuspensionPoll {
        if self.best.is_some() {
            self.success_result()
        } else {
            CommandResultSuspensionPoll::Ready(Err(structure_not_found(&self.query)))
        }
    }

    fn success_result(&self) -> CommandResultSuspensionPoll {
        let Some(best) = &self.best else {
            return CommandResultSuspensionPoll::Ready(Err(structure_not_found(&self.query)));
        };
        let pos = best.candidate.locate_pos;
        let distance = horizontal_distance(self.origin, pos);
        let structure_name = self.query.found_name(&best.found_structure);
        self.source.send_success(
            &locate_success_component(
                &translations::COMMANDS_LOCATE_STRUCTURE_SUCCESS,
                structure_name.clone(),
                pos,
                distance,
                false,
            ),
            false,
        );
        tracing::info!(
            "Locating element {} took {} ms",
            structure_name,
            self.started_at.elapsed().as_millis()
        );
        CommandResultSuspensionPoll::Ready(Ok(distance))
    }

    fn cancelled_result() -> CommandResultSuspensionPoll {
        CommandResultSuspensionPoll::Ready(Err(CommandSyntaxError::dynamic(
            "Structure search was cancelled",
        )))
    }
}

enum PendingRequest {
    Pending,
    Ready,
    Cancelled,
}

fn invalid_structure(query: &StructureOrTagKey) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(
        translations::COMMANDS_LOCATE_STRUCTURE_INVALID
            .message([TextComponent::from(query.as_printable())])
            .component(),
    )
}

fn structure_not_found(query: &StructureOrTagKey) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(
        translations::COMMANDS_LOCATE_STRUCTURE_NOT_FOUND
            .message([TextComponent::from(query.as_printable())])
            .component(),
    )
}

fn biome_not_found(query: &BiomeOrTag) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(
        translations::COMMANDS_LOCATE_BIOME_NOT_FOUND
            .message([TextComponent::from(query.as_printable())])
            .component(),
    )
}

fn poi_not_found(query: &PoiOrTag) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(
        translations::COMMANDS_LOCATE_POI_NOT_FOUND
            .message([TextComponent::from(query.as_printable())])
            .component(),
    )
}

fn horizontal_distance(a: BlockPos, b: BlockPos) -> i32 {
    let dx = b.0.x.wrapping_sub(a.0.x);
    let dz = b.0.z.wrapping_sub(a.0.z);
    let squared = dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz));
    (f64::from(squared).sqrt() as f32).floor() as i32
}

fn distance_3d(a: BlockPos, b: BlockPos) -> i32 {
    let dx = f64::from(b.0.x) - f64::from(a.0.x);
    let dy = f64::from(b.0.y) - f64::from(a.0.y);
    let dz = f64::from(b.0.z) - f64::from(a.0.z);
    let squared = dx * dx + dy * dy + dz * dz;
    // Vanilla narrows the squared distance to f32 before the square root.
    (f64::from(squared as f32).sqrt() as f32).floor() as i32
}

fn locate_success_component(
    translation: &Translation<3usize>,
    found_name: String,
    pos: BlockPos,
    distance: i32,
    include_y: bool,
) -> TextComponent {
    translation
        .message([
            TextComponent::from(found_name),
            locate_coordinates_component(pos, include_y),
            TextComponent::from(distance.to_string()),
        ])
        .component()
}

fn locate_coordinates_component(pos: BlockPos, include_y: bool) -> TextComponent {
    let displayed_y = if include_y {
        pos.0.y.to_string()
    } else {
        "~".to_owned()
    };
    TextComponent::plain("[")
        .add_child(
            translations::CHAT_COORDINATES
                .message([
                    TextComponent::from(pos.0.x.to_string()),
                    TextComponent::from(displayed_y.clone()),
                    TextComponent::from(pos.0.z.to_string()),
                ])
                .component(),
        )
        .add_child(TextComponent::plain("]"))
        .color(Color::Green)
        .hover_event(HoverEvent::show_text(
            &translations::CHAT_COORDINATES_TOOLTIP,
        ))
        .click_event(ClickEvent::suggest_command(format!(
            "/tp @s {} {} {}",
            pos.0.x, displayed_y, pos.0.z
        )))
}

#[cfg(test)]
mod tests {
    use super::super::create_dispatcher;
    use super::*;
    use crate::command::{
        brigadier::{CommandDispatcher, NodeId},
        execution::SteelCommandRuntime,
    };
    use steel_registry::init_vanilla_registry;

    type Dispatcher = CommandDispatcher<CommandSource, SteelCommandRuntime>;

    fn child(dispatcher: &Dispatcher, parent: NodeId, name: &str) -> NodeId {
        let Some(children) = dispatcher.children(parent) else {
            panic!("parent node should exist");
        };
        let Some(child) = children.iter().copied().find(|child| {
            dispatcher
                .node(*child)
                .is_some_and(|node| node.name() == name)
        }) else {
            panic!("child {name} should exist");
        };
        child
    }

    #[test]
    fn locate_graph_exposes_structure_biome_and_poi_branches() {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let locate = child(&dispatcher, dispatcher.root(), "locate");
        let structure = child(&dispatcher, locate, "structure");
        let structure_target = child(&dispatcher, structure, "structure");
        let biome = child(&dispatcher, locate, "biome");
        let biome_target = child(&dispatcher, biome, "biome");
        let poi = child(&dispatcher, locate, "poi");
        let poi_target = child(&dispatcher, poi, "poi");

        assert_eq!(
            dispatcher
                .node(structure_target)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::structure_or_tag_key())
        );
        assert_eq!(
            dispatcher
                .node(biome_target)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::biome_or_tag())
        );
        assert_eq!(
            dispatcher
                .node(poi_target)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::poi_or_tag())
        );
        for target in [structure_target, biome_target, poi_target] {
            let Some(target_node) = dispatcher.node(target) else {
                panic!("locate argument should exist");
            };
            assert!(target_node.is_executable());
            assert!(dispatcher.children(target).is_some_and(<[_]>::is_empty));
        }
        assert_eq!(dispatcher.children(locate).map(<[_]>::len), Some(3));
    }

    #[test]
    fn locate_coordinates_component_matches_vanilla_interactivity() {
        let component = locate_coordinates_component(BlockPos::new(12, 64, -34), true);

        assert_eq!(component.format.color, Some(Color::Green));
        assert!(matches!(
            component.interactions.click,
            Some(ClickEvent::SuggestCommand { ref command })
                if command.as_ref() == "/tp @s 12 64 -34"
        ));
        assert!(matches!(
            component.interactions.hover,
            Some(HoverEvent::ShowText { .. })
        ));

        let component = locate_coordinates_component(BlockPos::new(12, 64, -34), false);
        assert!(matches!(
            component.interactions.click,
            Some(ClickEvent::SuggestCommand { ref command })
                if command.as_ref() == "/tp @s 12 ~ -34"
        ));
    }

    #[test]
    fn horizontal_distance_matches_vanillas_wrapping_int_and_float_math() {
        assert_eq!(
            horizontal_distance(BlockPos::new(0, 0, 0), BlockPos::new(3, 100, 4)),
            5
        );
        assert_eq!(
            horizontal_distance(
                BlockPos::new(-30_000_000, 0, 0),
                BlockPos::new(30_000_000, 0, 0)
            ),
            36_907
        );
    }
}
