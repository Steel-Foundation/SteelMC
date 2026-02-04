//! A entity argument.
use crate::command::arguments::SuggestionContext;
use crate::command::context::CommandContext;
use crate::entity::Entity;
use crate::{command::arguments::CommandArgument, entity::LivingEntity};
use rand::seq::IteratorRandom;
use std::sync::Arc;
use steel_protocol::packets::game::{ArgumentType, SuggestionEntry, SuggestionType};
use steel_utils::translations::{
    ARGUMENT_ENTITY_SELECTOR_ALL_ENTITIES, ARGUMENT_ENTITY_SELECTOR_ALL_PLAYERS,
    ARGUMENT_ENTITY_SELECTOR_NEAREST_ENTITY, ARGUMENT_ENTITY_SELECTOR_NEAREST_PLAYER,
    ARGUMENT_ENTITY_SELECTOR_RANDOM_PLAYER, ARGUMENT_ENTITY_SELECTOR_SELF,
};
use uuid::Uuid;

/// A entity argument.
#[derive(Default)]
pub struct EntityArgument {
    /// If only accepts one entity
    one: bool,
}
impl EntityArgument {
    /// Creates a selector for multiple entities
    #[must_use]
    pub const fn new() -> Self {
        EntityArgument { one: false }
    }
    /// Creates a selector for one entity
    #[must_use]
    pub const fn one() -> Self {
        EntityArgument { one: true }
    }
}

impl CommandArgument for EntityArgument {
    type Output = Vec<Arc<dyn LivingEntity + Send + Sync>>;

    fn parse<'a>(
        &self,
        arg: &'a [&'a str],
        context: &mut CommandContext,
    ) -> Option<(&'a [&'a str], Self::Output)> {
        let players = context.server.get_players();
        if players.is_empty() {
            return Some((&arg[1..], vec![]));
        }
        let entities = match arg[0] {
            // TODO: Add getting entities
            "@a" | "@e" => players
                .into_iter()
                .map(|p| p as Arc<dyn LivingEntity + Send + Sync>)
                .collect(),
            "@n" | "@p" => {
                let position = context.position;
                let mut near_dist = (f64::MAX, players[0].clone());
                for player in players {
                    let dist = player.get_position().squared_distance_to_vec(position);
                    if dist < near_dist.0 {
                        near_dist = (dist, player);
                    }
                }
                vec![near_dist.1 as Arc<dyn LivingEntity + Send + Sync>]
            }
            "@r" => {
                vec![players.into_iter().choose(&mut rand::rng())?
                    as Arc<dyn LivingEntity + Send + Sync>]
            }
            "@s" => {
                if let Some(player) = &context.player {
                    vec![player.clone() as Arc<dyn LivingEntity + Send + Sync>]
                } else {
                    vec![]
                }
            }
            name => {
                let uuid = if let Ok(uuid) = Uuid::parse_str(name) {
                    uuid
                } else {
                    Uuid::nil()
                };
                let player = players.into_iter().find_map(|p| {
                    if p.gameprofile.name == name || p.uuid() == uuid {
                        Some(p)
                    } else {
                        None
                    }
                })?;
                vec![player as Arc<dyn LivingEntity + Send + Sync>]
            }
        };
        // TODO: Add entity argiments. (e.g. @e[limit=1])
        Some((&arg[1..], entities))
    }

    fn usage(&self) -> (ArgumentType, Option<SuggestionType>) {
        (
            ArgumentType::Entity {
                flags: u8::from(self.one),
            },
            Some(SuggestionType::AskServer),
        )
    }

    fn suggest(&self, prefix: &str, suggestion_ctx: &SuggestionContext) -> Vec<SuggestionEntry> {
        let mut suggestions = vec![
            SuggestionEntry::with_tooltip("@a", &ARGUMENT_ENTITY_SELECTOR_ALL_PLAYERS),
            SuggestionEntry::with_tooltip("@e", &ARGUMENT_ENTITY_SELECTOR_ALL_ENTITIES),
            SuggestionEntry::with_tooltip("@n", &ARGUMENT_ENTITY_SELECTOR_NEAREST_ENTITY),
            SuggestionEntry::with_tooltip("@p", &ARGUMENT_ENTITY_SELECTOR_NEAREST_PLAYER),
            SuggestionEntry::with_tooltip("@r", &ARGUMENT_ENTITY_SELECTOR_RANDOM_PLAYER),
            SuggestionEntry::with_tooltip("@s", &ARGUMENT_ENTITY_SELECTOR_SELF),
        ];
        suggestions.append(
            &mut suggestion_ctx
                .server
                .get_players()
                .iter()
                .map(|p| SuggestionEntry::new(p.gameprofile.name.clone()))
                .collect(),
        );
        suggestions.retain(|s| s.text.starts_with(prefix));
        suggestions
    }
}
