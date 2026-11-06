//! Ordered behavior chains replacing vanilla `super` call chains.
//!
//! A [`Pipeline`] is the flattened version of one overridable vanilla method
//! for one entity type: its stages run in order, base layer first, each one
//! recording which vanilla method it implements. Presets build a layer's
//! vanilla chain; entity types adjust it with [`Pipeline::after`],
//! [`Pipeline::before`], [`Pipeline::without`] and [`Pipeline::wrap`].
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

/// One overridable vanilla method: names the context its stages share.
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
    vanilla: &'static str,
    run: Run<E, K>,
}

impl<E: 'static, K: Entry> Stage<E, K> {
    /// A stage implementing the vanilla method named by `vanilla`
    /// (`"Animal.aiStep"`).
    #[must_use]
    pub const fn new(id: StageId, vanilla: &'static str, run: StageFn<E, K>) -> Self {
        Self {
            id,
            vanilla,
            run: Run::Plain(run),
        }
    }

    /// A stage that only acts on the entity and always continues; most
    /// vanilla overrides that call `super` first or last are this.
    #[must_use]
    pub const fn effect(id: StageId, vanilla: &'static str, run: fn(&E)) -> Self {
        Self {
            id,
            vanilla,
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

    fn origins(&self, out: &mut Vec<&'static str>) {
        if let Run::Wrap(_, inner) = &self.run {
            inner.origins(out);
        }
        if !out.contains(&self.vanilla) {
            out.push(self.vanilla);
        }
    }
}

/// The flattened chain of one vanilla method for one entity type.
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

    /// Removes stage `id`, for vanilla overrides that replace a layer without
    /// calling `super`. `vanilla_reason` documents which override does that.
    ///
    /// # Panics
    ///
    /// Panics if the pipeline has no stage `id`.
    #[must_use]
    #[track_caller]
    pub fn without(mut self, id: StageId, vanilla_reason: &'static str) -> Self {
        let _ = vanilla_reason;
        let at = self.index_of(id);
        self.stages.remove(at);
        self
    }

    /// Replaces stage `id` with `wrapper`, which decides when (and whether)
    /// the original stage runs: vanilla overrides that call `super` in the
    /// middle or conditionally.
    ///
    /// # Panics
    ///
    /// Panics if the pipeline has no stage `id`.
    #[must_use]
    #[track_caller]
    pub fn wrap(mut self, id: StageId, vanilla: &'static str, wrapper: WrapFn<E, K>) -> Self {
        let at = self.index_of(id);
        let inner = self.stages.remove(at);
        self.stages.insert(
            at,
            Stage {
                id,
                vanilla,
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

    /// The distinct vanilla methods this pipeline implements, in execution
    /// order. Compared against the flattener's executed chain in tests.
    #[must_use]
    pub fn origins(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for stage in &self.stages {
            stage.origins(&mut out);
        }
        out
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
        Pipeline::new([
            Stage::new(BASE, "Base.tick", base),
            Stage::new(MIDDLE, "Middle.tick", middle),
        ])
    }

    fn run(pipeline: &Pipeline<Probe, Log>) -> Vec<&'static str> {
        let mut log = Vec::new();
        pipeline.run(&PROBE, &mut log);
        log
    }

    #[test]
    fn insertions_and_removals_keep_vanilla_order() {
        let pipeline = preset()
            .before(BASE, [Stage::new(LEAF, "Leaf.tick", middle)])
            .without(MIDDLE, "Leaf.tick does not call super");
        assert_eq!(run(&pipeline), ["middle", "base"]);
        assert_eq!(pipeline.origins(), ["Leaf.tick", "Base.tick"]);
    }

    #[test]
    fn stop_skips_the_remaining_stages() {
        let pipeline = preset().before(MIDDLE, [Stage::new(LEAF, "Leaf.tick", guard)]);
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
        let pipeline = preset().wrap(BASE, "Leaf.tick", around);
        assert_eq!(run(&pipeline), ["before", "base", "after", "middle"]);
        assert_eq!(
            pipeline.origins(),
            ["Base.tick", "Leaf.tick", "Middle.tick"]
        );
    }

    #[test]
    #[should_panic(expected = "pipeline has no stage `test.leaf`")]
    fn positioning_against_a_missing_stage_fails_loudly() {
        let _ = preset().after(LEAF, []);
    }
}
