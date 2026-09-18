use steel_utils::Identifier;

pub struct ImpossibleInstance;
impl CriterionTriggerInstance for ImpossibleInstance {}

pub struct ImpossibleTrigger;
impl CriterionTrigger<ImpossibleInstance> for ImpossibleTrigger {
    fn id(&self) -> Identifier {
        Identifier::vanilla_static("impossible")
    }

    fn parse_instance(&self, _instance: String) -> ImpossibleInstance {
        ImpossibleInstance
    }
}

pub trait AnyCriterion: Send + Sync {
    fn id(&self) -> Identifier;
}

impl<T, G> AnyCriterion for Criterion<T, G>
where
    T: CriterionTriggerInstance + 'static,
    G: CriterionTrigger<T> + 'static,
{
    fn id(&self) -> Identifier {
        self.trigger.id()
    }
}

#[expect(dead_code)]
pub struct Criterion<
    T: CriterionTriggerInstance + 'static,
    G: CriterionTrigger<T> + ?Sized + 'static,
> {
    trigger: &'static G,
    instance: Box<T>,
}

static IMPOSSIBLE_TRIGGER: ImpossibleTrigger = ImpossibleTrigger;

impl<T: CriterionTriggerInstance, G: CriterionTrigger<T> + ?Sized> Criterion<T, G> {
    pub fn new(trigger: &'static G, instance: T) -> Self {
        Self {
            trigger,
            instance: Box::new(instance),
        }
    }
}

impl Default for Criterion<ImpossibleInstance, ImpossibleTrigger> {
    fn default() -> Self {
        Criterion {
            trigger: &IMPOSSIBLE_TRIGGER,
            instance: Box::new(ImpossibleInstance),
        }
    }
}

pub trait CriterionTrigger<T>: Send + Sync
where
    T: CriterionTriggerInstance,
{
    fn id(&self) -> Identifier;
    fn parse_instance(&self, instance: String) -> T;
}

pub trait CriterionTriggerInstance: Send + Sync {}
