//! Host side of [`World::fill`](crate::World::fill) and `World::replace_blocks`.
use crate::{
    BlockPos, BlockState,
    fill::{FillMode, FillReport, FillRequest, FillShapeKind, ReplaceFilter, ReplacedCount},
};
use std::sync::Arc;
use steel_core::world::{
    World as CoreWorld,
    bulk_fill::{self, FillPattern, FillShape},
};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_utils::BlockStateId;

const fn position(pos: BlockPos) -> steel_utils::BlockPos {
    steel_utils::BlockPos::new(pos.x, pos.y, pos.z)
}

fn core_request(request: &FillRequest) -> bulk_fill::FillRequest {
    let shape = match request.kind {
        FillShapeKind::Cuboid => FillShape::Cuboid {
            min: position(request.min),
            max: position(request.max),
        },
        FillShapeKind::Sphere => FillShape::Sphere {
            center: position(request.center),
            radius: request.radius,
        },
        FillShapeKind::Shell => FillShape::Shell {
            center: position(request.center),
            radius: request.radius,
            thickness: request.thickness,
        },
    };
    bulk_fill::FillRequest {
        shape,
        clip: request
            .clipped
            .then(|| (position(request.clip_min), position(request.clip_max))),
        pattern: FillPattern::Weighted(
            request
                .pattern
                .iter()
                .map(|entry| (BlockStateId(entry.state.id), entry.weight))
                .collect(),
        ),
        mode: match request.mode {
            FillMode::All => bulk_fill::FillMode::All,
            FillMode::OnlyAir => bulk_fill::FillMode::OnlyAir,
            FillMode::NonAir => bulk_fill::FillMode::NonAir,
        },
    }
}

pub(crate) fn fill(world: &Arc<CoreWorld>, request: &FillRequest) -> Result<FillReport, String> {
    let report = world
        .fill_blocks(&core_request(request))
        .map_err(|error| error.to_string())?;
    Ok(api_report(report))
}

pub(crate) fn replace_blocks(
    world: &Arc<CoreWorld>,
    min: BlockPos,
    max: BlockPos,
    filter: &ReplaceFilter,
    state: BlockState,
) -> Result<FillReport, String> {
    let states: Vec<BlockStateId> = filter
        .states
        .iter()
        .map(|state| BlockStateId(state.id))
        .collect();
    let replaceable = filter.replaceable;
    let report = world
        .replace_blocks(
            position(min),
            position(max),
            &|old| states.contains(&old) || (replaceable && old.is_replaceable()),
            BlockStateId(state.id),
        )
        .map_err(|error| error.to_string())?;
    Ok(api_report(report))
}

fn api_report(report: bulk_fill::FillReport) -> FillReport {
    let mut replaced: Vec<ReplacedCount> = report
        .replaced
        .into_iter()
        .map(|(state, count)| ReplacedCount {
            count,
            state: BlockState { id: state.0 },
        })
        .collect();
    replaced.sort_by(|a, b| b.count.cmp(&a.count).then(a.state.id.cmp(&b.state.id)));
    FillReport {
        replaced: replaced.into_iter().collect(),
        changed: report.changed,
        unchanged: report.unchanged,
        micros: u64::try_from(report.elapsed.as_micros()).unwrap_or(u64::MAX),
        chunks: report.chunks,
        sections: report.sections,
    }
}
