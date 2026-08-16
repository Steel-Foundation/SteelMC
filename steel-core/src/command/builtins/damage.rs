//! Vanilla damage entity command.

use super::super::{
    brigadier::{ArgumentType, CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};
use crate::entity::{SharedEntity, damage::DamageSource};
use steel_registry::vanilla_damage_types;
use steel_utils::Identifier;
use steel_utils::translations::{COMMANDS_DAMAGE_INVULNERABLE, COMMANDS_DAMAGE_SUCCESS};
use text_components::TextComponent;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("damage"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("damage").then(
        argument("target", SteelArgumentType::entity()).then(
            argument("amount", ArgumentType::float(0.0, f32::MAX))
                .executes(damage)
                .then(
                    argument("damageType", SteelArgumentType::damage_type())
                        .executes(damage)
                        .then(literal("at").then(
                            argument("location", SteelArgumentType::vec3(true)).executes(damage),
                        ))
                        .then(
                            literal("by").then(
                                argument("entity", SteelArgumentType::entity())
                                    .executes(damage)
                                    .then(
                                        literal("from").then(
                                            argument("cause", SteelArgumentType::entity())
                                                .executes(damage),
                                        ),
                                    ),
                            ),
                        ),
                ),
        ),
    )
}

fn damage(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let target = context.entity("target")?;
    let amount = context.float("amount")?;

    // The base damage type for this command is generic
    let damage_type = context
        .damage_type("damageType")
        .unwrap_or(&vanilla_damage_types::GENERIC);

    // Create the DamageSource to apply modifiers after
    let mut damage_source = DamageSource::environment(damage_type);

    // If we can get "location" from the context, it's from "at"
    if let Ok(coordinates) = context.coordinates("location") {
        damage_source = damage_source.with_source_position(coordinates.position(context.source()));
    }

    // Else, it's from the "by", or maybe it's nothing
    if context.argument("entity").is_ok() {
        let entity = context.entity("entity")?;
        let cause = if context.argument("cause").is_ok() {
            Some(context.entity("cause")?)
        } else {
            None
        };
        damage_source = with_damage_entities(damage_source, &entity, cause.as_ref());
    }

    if target.hurt(context.source().world(), &damage_source, amount) {
        context.source().send_success(
            &COMMANDS_DAMAGE_SUCCESS
                .message([
                    TextComponent::plain(format!("{amount:?}")),
                    target.display_name(),
                ])
                .component(),
            true,
        );
        Ok(1)
    } else {
        context
            .source()
            .send_failure(COMMANDS_DAMAGE_INVULNERABLE.msg().component());
        Ok(0)
    }
}

fn with_damage_entities(
    source: DamageSource,
    direct: &SharedEntity,
    cause: Option<&SharedEntity>,
) -> DamageSource {
    source
        .with_direct_entity_reference(direct)
        .with_causing_entity_reference(cause.unwrap_or(direct))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_damage_types, vanilla_entities};

    use super::*;
    use crate::entity::EntityOwnership;
    use crate::entity::entities::PigEntity;
    use crate::test_support::fresh_test_world_in_domain;
    use crate::world::World;

    fn pig(world: &Arc<World>, id: i32, position: DVec3) -> SharedEntity {
        Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            id,
            position,
            Arc::downgrade(world),
        ))
    }

    #[test]
    fn by_entity_uses_the_same_direct_and_causing_entity() {
        init_vanilla_registry();
        let world = fresh_test_world_in_domain("damage_command", "overworld");
        let entity = pig(&world, 1, DVec3::new(1.0, 64.0, 2.0));
        let source = with_damage_entities(
            DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK),
            &entity,
            None,
        );

        assert_eq!(source.direct_entity_id, Some(entity.id()));
        assert_eq!(source.causing_entity_id, Some(entity.id()));
        assert!(source.is_direct());
        assert!(Arc::ptr_eq(
            &source
                .direct_entity(&world)
                .expect("direct entity should resolve"),
            &entity,
        ));
        assert!(Arc::ptr_eq(
            &source
                .causing_entity(&world)
                .expect("causing entity should resolve"),
            &entity,
        ));
        assert_eq!(source.source_position_raw(), None);
        assert_eq!(
            source.effective_source_position(&world),
            Some(entity.position()),
        );
    }

    #[test]
    fn by_from_resolves_both_entities_across_same_domain_worlds() {
        init_vanilla_registry();
        let source_world = fresh_test_world_in_domain("damage_command_cross_world", "overworld");
        let target_world = fresh_test_world_in_domain("damage_command_cross_world", "the_nether");
        let direct = pig(&source_world, 2, DVec3::new(3.0, 64.0, 4.0));
        let cause = pig(&source_world, 3, DVec3::new(5.0, 64.0, 6.0));
        let source = with_damage_entities(
            DamageSource::environment(&vanilla_damage_types::ARROW),
            &direct,
            Some(&cause),
        );

        assert!(!source.is_direct());
        assert!(Arc::ptr_eq(
            &source
                .direct_entity(&target_world)
                .expect("same-domain direct entity should resolve"),
            &direct,
        ));
        assert!(Arc::ptr_eq(
            &source
                .causing_entity(&target_world)
                .expect("same-domain causing entity should resolve"),
            &cause,
        ));
    }

    #[test]
    fn entity_reference_never_falls_back_to_a_same_id_entity_in_another_domain() {
        init_vanilla_registry();
        let source_world = fresh_test_world_in_domain("damage_command_source_domain", "overworld");
        let target_world = fresh_test_world_in_domain("damage_command_target_domain", "overworld");
        let referenced = pig(&source_world, 4, DVec3::new(7.0, 64.0, 8.0));
        let same_id = pig(&target_world, referenced.id(), DVec3::new(9.0, 64.0, 10.0));
        assert_ne!(referenced.uuid(), same_id.uuid());
        target_world
            .entity_manager()
            .add_live_entity(same_id, EntityOwnership::External)
            .expect("same-ID target-domain entity should register");
        let source = with_damage_entities(
            DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK),
            &referenced,
            None,
        );

        assert!(source.direct_entity(&target_world).is_none());
        assert!(source.causing_entity(&target_world).is_none());
    }
}
