//! Handler for the "locate" command.

use std::sync::Arc;
use std::time::Instant;

use steel_utils::{BlockPos, ChunkPos, Identifier, translations};
use text_components::format::Color;
use text_components::interactivity::{ClickEvent, HoverEvent};
use text_components::{Modifier, TextComponent};

use crate::chunk::chunk_access::ChunkStatus;
use crate::chunk::chunk_request::{
    ChunkRequest, ChunkRequestHandle, ChunkRequestState, ChunkTicketKind,
};
use crate::command::arguments::structure::{StructureArgument, StructureArgumentValue};
use crate::command::commands::{CommandHandlerBuilder, CommandHandlerDyn, argument, literal};
use crate::command::context::CommandContext;
use crate::command::error::CommandError;
use crate::command::sender::CommandSender;
use crate::server::jobs::{JobPoll, ServerJob, ServerJobContext};
use crate::world::World;
use crate::worldgen::generator::ChunkGenerator;
use crate::worldgen::structure::{StructureLocateCandidate, StructureLocatePlan, squared_distance};

const MAX_STRUCTURE_LOCATE_RADIUS: i32 = 100;

/// Handler for the "locate" command.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["locate"],
        "Locates structures.",
        "minecraft:command.locate",
    )
    .then(
        literal("structure").then(argument("structure", StructureArgument).executes(
            |((), structure): ((), StructureArgumentValue),
             context: &mut CommandContext|
             -> Result<(), CommandError> { locate_structure(structure, context) },
        )),
    )
}

fn locate_structure(
    structure: StructureArgumentValue,
    context: &mut CommandContext,
) -> Result<(), CommandError> {
    let Some(structure_generator) = context
        .world
        .chunk_map
        .world_gen_context
        .generator
        .structure_generator()
    else {
        return Err(CommandError::CommandFailed(Box::new(TextComponent::plain(
            "Could not find any configured structures in this world",
        ))));
    };

    let structure_keys = structure.structure_keys();
    let query_name = structure.query_name();
    let Some(plan) = structure_generator.locate_plan_for_structures(&structure_keys) else {
        return Err(CommandError::CommandFailed(Box::new(TextComponent::plain(
            format!("Could not find any configured placements for {query_name}"),
        ))));
    };

    if plan.is_empty() {
        return Err(CommandError::CommandFailed(Box::new(TextComponent::plain(
            format!("Could not find any configured placements for {query_name}"),
        ))));
    }

    let origin = BlockPos::from(context.position);
    let job = LocateStructureJob {
        sender: context.sender.clone(),
        world: context.world.clone(),
        structures: structure_keys,
        query: structure,
        plan,
        origin,
        phase: LocatePhase::Start,
        pending: None,
        candidates: Vec::new(),
        best: None,
        random_radius: 0,
        started_at: Instant::now(),
    };
    context.server.jobs.spawn(job);
    Ok(())
}

enum LocatePhase {
    Start,
    WaitingRings,
    RandomSpread,
    WaitingRandomSpread,
    Finished,
}

#[derive(Clone)]
struct LocatedStructure {
    candidate: StructureLocateCandidate,
    found_structure: Identifier,
    distance_sqr: i64,
}

struct LocateStructureJob {
    sender: CommandSender,
    world: Arc<World>,
    structures: Vec<Identifier>,
    query: StructureArgumentValue,
    plan: StructureLocatePlan,
    origin: BlockPos,
    phase: LocatePhase,
    pending: Option<ChunkRequestHandle>,
    candidates: Vec<StructureLocateCandidate>,
    best: Option<LocatedStructure>,
    random_radius: i32,
    started_at: Instant,
}

impl ServerJob for LocateStructureJob {
    fn poll(&mut self, _context: &mut ServerJobContext) -> JobPoll {
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
                    return JobPoll::Pending;
                }
                LocatePhase::WaitingRings => match self.poll_pending_request() {
                    PendingRequest::Pending => return JobPoll::Pending,
                    PendingRequest::Cancelled => return JobPoll::Finished,
                    PendingRequest::Ready => {
                        self.best = self.first_valid_candidate();
                        self.clear_request();

                        if self.best.is_some() && !self.plan.has_random_spread() {
                            self.send_success();
                            self.phase = LocatePhase::Finished;
                            return JobPoll::Finished;
                        }

                        self.phase = LocatePhase::RandomSpread;
                    }
                },
                LocatePhase::RandomSpread => {
                    if self.random_radius > MAX_STRUCTURE_LOCATE_RADIUS {
                        self.finish_without_more_candidates();
                        return JobPoll::Finished;
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
                    return JobPoll::Pending;
                }
                LocatePhase::WaitingRandomSpread => match self.poll_pending_request() {
                    PendingRequest::Pending => return JobPoll::Pending,
                    PendingRequest::Cancelled => return JobPoll::Finished,
                    PendingRequest::Ready => {
                        if let Some(found) = self.best_after_random_radius_if_found() {
                            self.best = Some(found);
                            self.send_success();
                            self.phase = LocatePhase::Finished;
                            return JobPoll::Finished;
                        }

                        self.clear_request();
                        self.phase = LocatePhase::RandomSpread;
                    }
                },
                LocatePhase::Finished => return JobPoll::Finished,
            }
        }
    }

    fn cancel(&mut self) {
        if let Some(pending) = &mut self.pending {
            pending.cancel();
        }
    }
}

impl LocateStructureJob {
    fn request_current_candidates(&self) -> ChunkRequestHandle {
        let positions: Vec<ChunkPos> = self
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

    fn poll_pending_request(&mut self) -> PendingRequest {
        let Some(pending) = &mut self.pending else {
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

    fn best_after_random_radius_if_found(&self) -> Option<LocatedStructure> {
        let mut best = self.best.clone();
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

        if found_in_this_radius { best } else { None }
    }

    fn generated_structure_at_candidate(
        &self,
        candidate: StructureLocateCandidate,
    ) -> Option<Identifier> {
        let holder = self
            .world
            .chunk_map
            .chunks
            .read_sync(&candidate.chunk_pos, |_, holder| holder.clone())?;
        let chunk = holder.try_chunk(ChunkStatus::StructureStarts)?;
        let starts = chunk.structure_starts();
        self.structures.iter().find_map(|structure| {
            starts
                .get(structure)
                .is_some_and(|start| !start.pieces.is_empty())
                .then(|| structure.clone())
        })
    }

    fn finish_without_more_candidates(&mut self) {
        if self.best.is_some() {
            self.send_success();
        } else {
            self.send_not_found();
        }
        self.phase = LocatePhase::Finished;
    }

    fn send_success(&self) {
        let Some(best) = &self.best else {
            return;
        };
        let pos = best.candidate.locate_pos;
        let distance = horizontal_distance(self.origin, pos);
        let structure_name = self.query.printable_name(&best.found_structure);
        self.sender.send_message(&locate_success_component(
            structure_name.clone(),
            pos,
            distance,
        ));
        tracing::info!(
            "Locating structure {} took {} ms",
            structure_name,
            self.started_at.elapsed().as_millis()
        );
    }

    fn send_not_found(&self) {
        self.sender.send_message(
            &translations::COMMANDS_LOCATE_STRUCTURE_NOT_FOUND
                .message([TextComponent::from(self.query.query_name())])
                .component(),
        );
        tracing::info!(
            "Locating structure {} failed after {} ms",
            self.query.query_name(),
            self.started_at.elapsed().as_millis()
        );
    }
}

enum PendingRequest {
    Pending,
    Ready,
    Cancelled,
}

fn horizontal_distance(a: BlockPos, b: BlockPos) -> i32 {
    let dx = (i64::from(a.0.x) - i64::from(b.0.x)) as f64;
    let dz = (i64::from(a.0.z) - i64::from(b.0.z)) as f64;
    (dx.mul_add(dx, dz * dz).sqrt().floor()) as i32
}

fn locate_success_component(structure_name: String, pos: BlockPos, distance: i32) -> TextComponent {
    translations::COMMANDS_LOCATE_STRUCTURE_SUCCESS
        .message([
            TextComponent::from(structure_name),
            locate_coordinates_component(pos),
            TextComponent::from(distance.to_string()),
        ])
        .component()
}

fn locate_coordinates_component(pos: BlockPos) -> TextComponent {
    let displayed_y = "~";
    TextComponent::plain("[")
        .add_child(
            translations::CHAT_COORDINATES
                .message([
                    TextComponent::from(pos.0.x.to_string()),
                    TextComponent::from(displayed_y),
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
    use super::*;

    #[test]
    fn locate_coordinates_component_matches_vanilla_interactivity() {
        let component = locate_coordinates_component(BlockPos::new(12, 0, -34));

        assert_eq!(component.format.color, Some(Color::Green));
        assert!(matches!(
            component.interactions.click,
            Some(ClickEvent::SuggestCommand { ref command })
                if command.as_ref() == "/tp @s 12 ~ -34"
        ));
        assert!(matches!(
            component.interactions.hover,
            Some(HoverEvent::ShowText { .. })
        ));
    }
}
