use std::{fmt, fs};

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::quote;
use serde::de::{Deserializer, MapAccess, Visitor};

struct GameEventEntry {
    name: String,
    notification_radius: i32,
}

struct GameEventsVisitor;

impl<'de> Visitor<'de> for GameEventsVisitor {
    type Value = Vec<GameEventEntry>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a game event object keyed by identifier")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut events = Vec::with_capacity(map.size_hint().unwrap_or_default());
        while let Some((name, notification_radius)) = map.next_entry()? {
            events.push(GameEventEntry {
                name,
                notification_radius,
            });
        }
        Ok(events)
    }
}

fn parse_game_events(json_content: &str) -> Vec<GameEventEntry> {
    let mut deserializer = serde_json::Deserializer::from_str(json_content);
    deserializer
        .deserialize_map(GameEventsVisitor)
        .expect("Failed to parse game_events.json")
}

pub fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/game_events.json");

    let json_content = fs::read_to_string("build_assets/game_events.json")
        .expect("Failed to read game_events.json");
    let game_events_file = parse_game_events(&json_content);

    let mut constants = TokenStream::new();

    let mut registrations = TokenStream::new();

    for GameEventEntry {
        name,
        notification_radius,
    } in game_events_file
    {
        let name = name.strip_prefix("minecraft:").unwrap_or(&name);
        let ident = Ident::new(&name.to_shouty_snake_case(), Span::call_site());
        let key = Literal::string(name);

        constants.extend(quote! {
            pub static #ident: GameEvent = GameEvent {
                key: Identifier::vanilla_static(#key),
                notification_radius: #notification_radius
            };
        });

        registrations.extend(quote! {
            registry.register(&#ident);
        });
    }

    quote! {
        use crate::game_events::{GameEvent, GameEventRegistry};
        use steel_utils::Identifier;

        #constants

        pub fn register_game_events(registry: &mut GameEventRegistry) {
            #registrations
        }
    }
}
