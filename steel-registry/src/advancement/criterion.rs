use steel_utils::Identifier;

pub struct Criterion<T> {
    trigger: &'static dyn CriterionTrigger<T>,
    instance: T,
}

struct ImpossibleTrigger;

struct ImpossibleInstance;
impl CriterionTriggerInstance for ImpossibleTrigger {}
impl CriterionTrigger<ImpossibleInstance> for ImpossibleTrigger {
    fn id(&self) -> Identifier {
        Identifier::vanilla_static("impossible")
    }

    fn parse_instance(_instance: String) -> ImpossibleInstance {
        ImpossibleInstance
    }
}

static IMPOSSIBLE_TRIGGER: ImpossibleTrigger = ImpossibleTrigger;

impl Default for Criterion<_> {
    const fn default() -> Self {
        Self {
            trigger: &IMPOSSIBLE_TRIGGER,
            instance: ImpossibleInstance,
        }
    }
}

impl<T> Criterion<T> {
    pub fn new(trigger: impl CriterionTrigger<T>, instance: T) -> Self {
        Self { trigger, instance }
    }
}

pub trait CriterionTrigger<T>
where
    T: CriterionTriggerInstance,
{
    fn id(&self) -> Identifier;
    fn parse_instance(instance: String) -> T;
}

pub trait CriterionTriggerInstance {}