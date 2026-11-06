//! Chicken's pipelines against vanilla's executed `super` chains, as printed
//! by `tools/class-flattener Chicken -s -m <method>`, in execution order.

use super::*;

#[test]
fn ai_step_runs_the_vanilla_chain() {
    // `Mob.aiStep` still runs `LivingEntity.aiStep` itself until the living
    // layer is a stage.
    assert_eq!(
        ChickenEntity::definition().ai_step.origins(),
        [
            "Mob.aiStep",
            "AgeableMob.aiStep",
            "Animal.aiStep",
            "Chicken.aiStep"
        ]
    );
}

#[test]
fn finalize_spawn_runs_the_vanilla_chain() {
    assert_eq!(
        ChickenEntity::definition().finalize_spawn.origins(),
        [
            "Chicken.finalizeSpawn",
            "AgeableMob.finalizeSpawn",
            "Mob.finalizeSpawn"
        ]
    );
}

#[test]
fn custom_server_ai_step_runs_the_vanilla_chain() {
    // `Mob.customServerAiStep` is empty.
    assert_eq!(
        ChickenEntity::definition().custom_server_ai_step.origins(),
        ["Animal.customServerAiStep"]
    );
}

#[test]
fn mob_interact_runs_the_vanilla_chain() {
    // `Mob.mobInteract` only returns `PASS`, the context's initial result.
    assert_eq!(
        ChickenEntity::definition().mob_interact.origins(),
        ["Animal.mobInteract", "AgeableMob.mobInteract"]
    );
}
