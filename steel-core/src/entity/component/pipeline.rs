//! Ordered behavior chains for entity entry points such as `ai_step`.
//!
//! A [`Pipeline`] is one entity type's behavior for one entry point, as an
//! ordered list of stages. Layer presets build the shared chain; entity types
//! adjust it with [`Pipeline::after`], [`Pipeline::before`],
//! [`Pipeline::without`] and [`Pipeline::wrap`].
//!
//! Stages return [`Flow::Stop`] to end the chain early, which covers vanilla
//! guards (`if (...) return false;` before `super`) and chain-of-responsibility
//! methods such as `mobInteract`.

use std::fmt;

/// Whether the remaining stages of a pipeline run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Run the next stage.
    Continue,
    /// Skip every remaining stage.
    Stop,
}

/// Identifies a stage so entity types can position their own stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageId(pub &'static str);

impl fmt::Display for StageId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

/// One entry point: names the context its stages share.
pub trait Entry: 'static {
    /// Arguments and result of one call, borrowed for the call.
    type Ctx<'a>;
}

type StageFn<E, K> = for<'a, 'b> fn(&'b E, &'b mut <K as Entry>::Ctx<'a>) -> Flow;

/// Continues a wrapped stage from inside its wrapper.
pub type Inner<'s, E, K> = &'s dyn for<'a, 'b> Fn(&'b E, &'b mut <K as Entry>::Ctx<'a>) -> Flow;

type WrapFn<E, K> =
    for<'a, 'b, 's> fn(&'b E, &'b mut <K as Entry>::Ctx<'a>, Inner<'s, E, K>) -> Flow;

enum Run<E: 'static, K: Entry> {
    Plain(StageFn<E, K>),
    Effect(fn(&E)),
    Wrap(WrapFn<E, K>, Box<Stage<E, K>>),
}

/// One step of a pipeline.
pub struct Stage<E: 'static, K: Entry> {
    id: StageId,
    run: Run<E, K>,
}

impl<E: 'static, K: Entry> Stage<E, K> {
    /// A stage that reads and writes the call's context and decides whether
    /// the pipeline continues.
    #[must_use]
    pub const fn new(id: StageId, run: StageFn<E, K>) -> Self {
        Self {
            id,
            run: Run::Plain(run),
        }
    }

    /// A stage that only acts on the entity and always continues.
    #[must_use]
    pub const fn effect(id: StageId, run: fn(&E)) -> Self {
        Self {
            id,
            run: Run::Effect(run),
        }
    }

    /// Returns this stage's id.
    #[must_use]
    pub const fn id(&self) -> StageId {
        self.id
    }

    fn call(&self, entity: &E, ctx: &mut K::Ctx<'_>) -> Flow {
        match &self.run {
            Run::Plain(run) => run(entity, ctx),
            Run::Effect(run) => {
                run(entity);
                Flow::Continue
            }
            Run::Wrap(wrapper, inner) => {
                wrapper(entity, ctx, &|entity, ctx| inner.call(entity, ctx))
            }
        }
    }
}

/// The ordered stages one entity type runs for one entry point.
pub struct Pipeline<E: 'static, K: Entry> {
    stages: Vec<Stage<E, K>>,
}

impl<E: 'static, K: Entry> Pipeline<E, K> {
    /// A pipeline running `stages` in order.
    #[must_use]
    pub fn new(stages: impl IntoIterator<Item = Stage<E, K>>) -> Self {
        Self {
            stages: stages.into_iter().collect(),
        }
    }

    #[track_caller]
    fn index_of(&self, id: StageId) -> usize {
        self.stages
            .iter()
            .position(|stage| stage.id == id)
            .unwrap_or_else(|| panic!("pipeline has no stage `{id}`"))
    }

    /// Inserts `stages` right after stage `id`.
    ///
    /// # Panics
    ///
    /// Panics if the pipeline has no stage `id`. Definitions are built once
    /// per entity type, so this fails at startup and in tests.
    #[must_use]
    #[track_caller]
    pub fn after(mut self, id: StageId, stages: impl IntoIterator<Item = Stage<E, K>>) -> Self {
        let at = self.index_of(id) + 1;
        self.stages.splice(at..at, stages);
        self
    }

    /// Inserts `stages` right before stage `id`.
    ///
    /// # Panics
    ///
    /// Panics if the pipeline has no stage `id`.
    #[must_use]
    #[track_caller]
    pub fn before(mut self, id: StageId, stages: impl IntoIterator<Item = Stage<E, K>>) -> Self {
        let at = self.index_of(id);
        self.stages.splice(at..at, stages);
        self
    }

    /// Appends `stages` after every existing stage.
    #[must_use]
    pub fn then(mut self, stages: impl IntoIterator<Item = Stage<E, K>>) -> Self {
        self.stages.extend(stages);
        self
    }

    /// Removes stage `id`, for entity types that replace a layer's behavior
    /// entirely.
    ///
    /// # Panics
    ///
    /// Panics if the pipeline has no stage `id`.
    #[must_use]
    #[track_caller]
    pub fn without(mut self, id: StageId) -> Self {
        let at = self.index_of(id);
        self.stages.remove(at);
        self
    }

    /// Replaces stage `id` with `wrapper`, which decides when (and whether)
    /// the original stage runs, for behavior that surrounds a layer or runs
    /// it conditionally.
    ///
    /// # Panics
    ///
    /// Panics if the pipeline has no stage `id`.
    #[must_use]
    #[track_caller]
    pub fn wrap(mut self, id: StageId, wrapper: WrapFn<E, K>) -> Self {
        let at = self.index_of(id);
        let inner = self.stages.remove(at);
        self.stages.insert(
            at,
            Stage {
                id,
                run: Run::Wrap(wrapper, Box::new(inner)),
            },
        );
        self
    }

    /// Runs the stages in order until one returns [`Flow::Stop`].
    pub fn run(&self, entity: &E, ctx: &mut K::Ctx<'_>) {
        for stage in &self.stages {
            if stage.call(entity, ctx) == Flow::Stop {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Entry, Flow, Inner, Pipeline, Stage, StageId};

    struct Log;

    /// Stand-in entity: stages only see it by reference, like real entities.
    struct Probe {
        _not_zero_sized: u64,
    }

    const PROBE: Probe = Probe { _not_zero_sized: 0 };

    impl Entry for Log {
        type Ctx<'a> = Vec<&'static str>;
    }

    const BASE: StageId = StageId("test.base");
    const MIDDLE: StageId = StageId("test.middle");
    const LEAF: StageId = StageId("test.leaf");

    fn base(_: &Probe, log: &mut Vec<&'static str>) -> Flow {
        log.push("base");
        Flow::Continue
    }

    fn middle(_: &Probe, log: &mut Vec<&'static str>) -> Flow {
        log.push("middle");
        Flow::Continue
    }

    fn guard(_: &Probe, log: &mut Vec<&'static str>) -> Flow {
        log.push("guard");
        Flow::Stop
    }

    fn preset() -> Pipeline<Probe, Log> {
        Pipeline::new([Stage::new(BASE, base), Stage::new(MIDDLE, middle)])
    }

    fn run(pipeline: &Pipeline<Probe, Log>) -> Vec<&'static str> {
        let mut log = Vec::new();
        pipeline.run(&PROBE, &mut log);
        log
    }

    #[test]
    fn insertions_and_removals_keep_the_preset_order() {
        let pipeline = preset()
            .before(BASE, [Stage::new(LEAF, middle)])
            .without(MIDDLE);
        assert_eq!(run(&pipeline), ["middle", "base"]);
    }

    #[test]
    fn stop_skips_the_remaining_stages() {
        let pipeline = preset().before(MIDDLE, [Stage::new(LEAF, guard)]);
        assert_eq!(run(&pipeline), ["base", "guard"]);
    }

    #[test]
    fn wrap_runs_the_original_where_the_wrapper_calls_it() {
        fn around(
            entity: &Probe,
            log: &mut Vec<&'static str>,
            inner: Inner<'_, Probe, Log>,
        ) -> Flow {
            log.push("before");
            let flow = inner(entity, log);
            log.push("after");
            flow
        }
        let pipeline = preset().wrap(BASE, around);
        assert_eq!(run(&pipeline), ["before", "base", "after", "middle"]);
    }

    #[test]
    #[should_panic(expected = "pipeline has no stage `test.leaf`")]
    fn positioning_against_a_missing_stage_fails_loudly() {
        let _ = preset().after(LEAF, []);
    }
}
