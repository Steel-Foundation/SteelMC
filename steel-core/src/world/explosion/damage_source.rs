//! Vanilla explosion damage-source construction.

use steel_registry::vanilla_damage_types;

use crate::entity::damage::DamageSource;
use crate::entity::{Entity, SharedEntity};

pub(super) fn default_explosion_damage_source(
    direct: Option<&dyn Entity>,
    indirect: Option<&dyn Entity>,
) -> DamageSource {
    let damage_type = if direct.is_some() && indirect.is_some() {
        &vanilla_damage_types::PLAYER_EXPLOSION
    } else {
        &vanilla_damage_types::EXPLOSION
    };
    let mut source = DamageSource::environment(damage_type);
    if let Some(entity) = direct {
        source = source
            .with_direct_entity(entity.id())
            .with_direct_entity_position(entity.position());
    }
    if let Some(entity) = indirect {
        source = source.with_causing_entity(entity.id());
    }
    source
}

pub(crate) fn default_explosion_damage_source_with_references(
    direct: &SharedEntity,
    indirect: Option<&SharedEntity>,
) -> DamageSource {
    let indirect_entity = indirect.map(|entity| entity.as_ref() as &dyn Entity);
    let mut source = default_explosion_damage_source(Some(direct.as_ref()), indirect_entity)
        .with_direct_entity_reference(direct);
    if let Some(indirect) = indirect {
        source = source.with_causing_entity_reference(indirect);
    }
    source
}
