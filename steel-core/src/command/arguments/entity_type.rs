//! Entity type command arguments.
use steel_protocol::packets::game::{ArgumentType, SuggestionEntry, SuggestionType};
use steel_registry::{REGISTRY, RegistryExt, entity_type::EntityTypeRef};
use steel_utils::Identifier;

use crate::command::{
    arguments::{CommandArgument, SuggestionContext},
    context::CommandContext,
};
use crate::entity::ENTITIES;

/// A vanilla `EntitySummonArgument`, restricted to summonable entity types.
pub struct EntitySummonArgument;

impl EntitySummonArgument {
    fn parse_identifier(input: &str) -> Option<Identifier> {
        let (namespace, path) = input.split_once(':').map_or(
            (Identifier::VANILLA_NAMESPACE, input),
            |(namespace, path)| (namespace, path),
        );

        Identifier::validate(namespace, path)
            .then(|| Identifier::new(namespace.to_owned(), path.to_owned()))
    }

    fn resolve(input: &str) -> Option<EntityTypeRef> {
        let key = Self::parse_identifier(input)?;
        REGISTRY
            .entity_types
            .by_key(&key)
            .filter(|entity_type| Self::can_summon(entity_type))
    }

    fn can_summon(entity_type: EntityTypeRef) -> bool {
        entity_type.summonable
            && ENTITIES
                .get()
                .is_some_and(|registry| registry.has_factory(entity_type))
    }
}

impl CommandArgument for EntitySummonArgument {
    type Output = EntityTypeRef;

    fn parse<'a>(
        &self,
        arg: &'a [&'a str],
        _context: &mut CommandContext,
    ) -> Option<(&'a [&'a str], Self::Output)> {
        Self::resolve(arg.first()?).map(|entity_type| (&arg[1..], entity_type))
    }

    fn usage(&self) -> (ArgumentType, Option<SuggestionType>) {
        (
            ArgumentType::Resource {
                identifier: "minecraft:entity_type",
            },
            Some(SuggestionType::SummonableEntities),
        )
    }

    fn suggest(&self, prefix: &str, _suggestion_ctx: &SuggestionContext) -> Vec<SuggestionEntry> {
        let stripped_prefix = prefix.strip_prefix("minecraft:").unwrap_or(prefix);
        REGISTRY
            .entity_types
            .iter()
            .filter(|(_, entity_type)| Self::can_summon(entity_type))
            .map(|(_, entity_type)| SuggestionEntry::new(entity_type.key.to_string()))
            .filter(|suggestion| {
                suggestion
                    .text
                    .strip_prefix("minecraft:")
                    .unwrap_or(&suggestion.text)
                    .starts_with(stripped_prefix)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::vanilla_entities;

    use super::*;
    use crate::entity::init_test_entities;

    #[test]
    fn resolves_summonable_entity_with_default_namespace() {
        init_test_entities();

        assert_eq!(
            EntitySummonArgument::resolve("pig"),
            Some(&vanilla_entities::PIG)
        );
    }

    #[test]
    fn resolves_summonable_entity_with_explicit_namespace() {
        init_test_entities();

        assert_eq!(
            EntitySummonArgument::resolve("minecraft:pig"),
            Some(&vanilla_entities::PIG)
        );
    }

    #[test]
    fn rejects_non_summonable_entity_type() {
        init_test_entities();

        assert_eq!(EntitySummonArgument::resolve("player"), None);
    }

    #[test]
    fn rejects_unknown_entity_type() {
        init_test_entities();

        assert_eq!(EntitySummonArgument::resolve("minecraft:not_real"), None);
    }
}
