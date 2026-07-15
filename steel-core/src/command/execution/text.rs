//! Resolution of command-context text components.

use simdnbt::owned::NbtTag;
use steel_utils::{Identifier, nbt::parse_nbt_path, text::command_nbt_component, translations};
use text_components::{
    TextComponent,
    content::{Content, NbtSource, Object, Resolvable},
    custom::CustomData,
    interactivity::HoverEvent,
    resolving::TryTextResolutor,
};

use super::{CommandSource, coordinates::parse_block_pos, selector::parse_entity_selector_text};
use crate::{
    command::brigadier::{CommandSyntaxError, StringReader},
    entity::Entity,
    scoreboard::ScoreHolder,
};

pub(crate) trait CommandTextResolutionSource {
    fn selector_display_names(
        &self,
        selector: &str,
    ) -> Result<Vec<TextComponent>, CommandSyntaxError>;

    fn score_selector_names(
        &self,
        selector: &str,
    ) -> Result<Option<Vec<String>>, CommandSyntaxError>;

    fn score(&self, holder: &str, objective: &str) -> Result<Option<i32>, CommandSyntaxError>;

    fn nbt_source(&self, source: &NbtSource) -> Result<Vec<NbtTag>, CommandSyntaxError>;
}

/// Resolves selectors, scores, and NBT against one command source.
pub(crate) struct CommandTextResolver<'a, S: ?Sized = CommandSource> {
    source: &'a S,
    default_scoreboard_name: Option<String>,
}

impl<'a> CommandTextResolver<'a, CommandSource> {
    pub(crate) fn with_entity_override(source: &'a CommandSource, entity: &dyn Entity) -> Self {
        Self {
            source,
            default_scoreboard_name: Some(entity.scoreboard_name()),
        }
    }
}

impl<S> TryTextResolutor for CommandTextResolver<'_, S>
where
    S: CommandTextResolutionSource + ?Sized,
{
    type Error = CommandSyntaxError;

    fn try_resolve_content(
        &self,
        resolvable: &Resolvable,
        recursion_depth: usize,
    ) -> Result<TextComponent, Self::Error> {
        match resolvable {
            Resolvable::Entity {
                selector,
                separator,
            } => {
                let values = self.source.selector_display_names(selector)?;
                let separator = separator
                    .as_deref()
                    .cloned()
                    .unwrap_or_else(|| *Resolvable::entity_separator());
                Ok(join_components(values, &separator))
            }
            Resolvable::Scoreboard {
                selector,
                objective,
            } => self.resolve_score(selector, objective),
            Resolvable::NBT {
                path,
                interpret,
                plain,
                separator,
                source,
            } => self.resolve_nbt(
                path,
                *interpret,
                *plain,
                separator.as_deref(),
                source,
                recursion_depth,
            ),
        }
    }

    fn try_resolve_custom(&self, data: &CustomData) -> Result<Option<TextComponent>, Self::Error> {
        Ok(Some(TextComponent::from(data.clone())))
    }
}

impl<S> CommandTextResolver<'_, S>
where
    S: CommandTextResolutionSource + ?Sized,
{
    fn resolve_score(
        &self,
        selector: &str,
        objective: &str,
    ) -> Result<TextComponent, CommandSyntaxError> {
        let mut holder = match self.source.score_selector_names(selector)? {
            Some(names) if names.len() > 1 => {
                return Err(CommandSyntaxError::dynamic(TextComponent::from(
                    &translations::ARGUMENT_ENTITY_TOOMANY,
                )));
            }
            Some(mut names) => names.pop().unwrap_or_else(|| selector.to_owned()),
            None => selector.to_owned(),
        };
        if holder == "*"
            && let Some(default_name) = &self.default_scoreboard_name
        {
            default_name.clone_into(&mut holder);
        }

        Ok(self
            .source
            .score(&holder, objective)?
            .map_or_else(TextComponent::new, |score| {
                TextComponent::plain(score.to_string())
            }))
    }

    fn resolve_nbt(
        &self,
        path: &str,
        interpret: bool,
        plain: bool,
        separator: Option<&TextComponent>,
        source: &NbtSource,
        recursion_depth: usize,
    ) -> Result<TextComponent, CommandSyntaxError> {
        let path = parse_nbt_path(path).map_err(|error| {
            CommandSyntaxError::dynamic(format!("Invalid NBT path '{path}': {error}"))
        })?;
        let selected = self
            .source
            .nbt_source(source)?
            .into_iter()
            .flat_map(|tag| path.get(&tag));
        let separator = separator
            .cloned()
            .unwrap_or_else(|| *Resolvable::nbt_separator());

        if !interpret {
            return Ok(join_components(
                selected.map(|tag| command_nbt_component(&tag, plain)),
                &separator,
            ));
        }

        let mut values = Vec::new();
        for tag in selected {
            let component = match TextComponent::try_from_nbt(&tag) {
                Ok(component) => component,
                Err(error) => {
                    tracing::warn!(?tag, %error, "failed to parse component from command NBT");
                    continue;
                }
            };
            if let Err(error) = validate_component_syntax(&component) {
                tracing::warn!(?tag, %error, "failed to compile component from command NBT");
                continue;
            }
            match component.try_resolve_from_depth(self, recursion_depth) {
                Ok(component) => values.push(component),
                Err(error) => {
                    tracing::warn!(?tag, %error, "failed to resolve component from command NBT");
                }
            }
        }
        Ok(join_components(values, &separator))
    }
}

impl CommandTextResolutionSource for CommandSource {
    fn selector_display_names(
        &self,
        selector: &str,
    ) -> Result<Vec<TextComponent>, CommandSyntaxError> {
        Ok(parse_entity_selector_text(selector)?
            .find_entities(self)?
            .into_iter()
            .map(|entity| entity.display_name())
            .collect())
    }

    fn score_selector_names(
        &self,
        selector: &str,
    ) -> Result<Option<Vec<String>>, CommandSyntaxError> {
        let Ok(selector) = parse_entity_selector_text(selector) else {
            return Ok(None);
        };
        Ok(Some(
            selector
                .find_entities(self)?
                .into_iter()
                .map(|entity| entity.scoreboard_name())
                .collect(),
        ))
    }

    fn score(&self, holder: &str, objective: &str) -> Result<Option<i32>, CommandSyntaxError> {
        let scoreboard = self
            .server()
            .scoreboards
            .get(self.world().domain())
            .ok_or_else(|| {
                CommandSyntaxError::dynamic(format!(
                    "Domain '{}' has no command scoreboard",
                    self.world().domain()
                ))
            })?;
        let Some(objective) = scoreboard.objective(objective) else {
            return Ok(None);
        };
        Ok(scoreboard.score(&ScoreHolder::new(holder), &objective))
    }

    fn nbt_source(&self, source: &NbtSource) -> Result<Vec<NbtTag>, CommandSyntaxError> {
        match source {
            NbtSource::Entity(selector) => Ok(parse_entity_selector_text(selector)?
                .find_entities(self)?
                .into_iter()
                .map(|entity| NbtTag::Compound(entity.nbt_for_data_compare()))
                .collect()),
            NbtSource::Block(coordinates) => {
                let coordinates = parse_block_coordinates(coordinates)?;
                let Some(block_entity) = self.world().get_block_entity(coordinates.block_pos(self))
                else {
                    return Ok(Vec::new());
                };
                Ok(vec![NbtTag::Compound(
                    block_entity.lock().save_with_full_metadata(),
                )])
            }
            NbtSource::Storage(identifier) => {
                let identifier = parse_resource_identifier(identifier).map_err(|error| {
                    CommandSyntaxError::dynamic(format!(
                        "Invalid command storage identifier '{identifier}': {error}"
                    ))
                })?;
                let storage = self
                    .server()
                    .command_storage
                    .get(self.world().domain())
                    .ok_or_else(|| {
                        CommandSyntaxError::dynamic(format!(
                            "Domain '{}' has no command storage",
                            self.world().domain()
                        ))
                    })?;
                Ok(vec![NbtTag::Compound(storage.get(&identifier))])
            }
        }
    }
}

fn parse_block_coordinates(raw: &str) -> Result<super::Coordinates, CommandSyntaxError> {
    let mut reader = StringReader::new(raw);
    let coordinates = parse_block_pos(&mut reader)?;
    if reader.can_read() {
        return Err(CommandSyntaxError::dynamic(format!(
            "Invalid block coordinates '{raw}': trailing data"
        )));
    }
    Ok(coordinates)
}

fn parse_resource_identifier(raw: &str) -> Result<Identifier, &'static str> {
    let (namespace, path) =
        raw.split_once(':')
            .map_or((Identifier::VANILLA_NAMESPACE, raw), |(namespace, path)| {
                if namespace.is_empty() {
                    (Identifier::VANILLA_NAMESPACE, path)
                } else {
                    (namespace, path)
                }
            });
    if namespace.is_empty() || path.is_empty() || !Identifier::validate(namespace, path) {
        return Err("invalid resource location");
    }
    Ok(Identifier::new(namespace.to_owned(), path.to_owned()))
}

fn join_components(
    values: impl IntoIterator<Item = TextComponent>,
    separator: &TextComponent,
) -> TextComponent {
    let mut values = values.into_iter();
    let Some(first) = values.next() else {
        return TextComponent::new();
    };
    let Some(second) = values.next() else {
        return first;
    };

    let mut result = TextComponent::new();
    result.children.push(first);
    result.children.push(separator.clone());
    result.children.push(second);
    for value in values {
        result.children.push(separator.clone());
        result.children.push(value);
    }
    result
}

/// Validates component strings that vanilla compiles as part of its component codec.
pub(super) fn validate_component_syntax(component: &TextComponent) -> Result<(), String> {
    match &component.content {
        Content::Translate(message) => {
            if let Some(arguments) = &message.args {
                for argument in arguments {
                    validate_component_syntax(argument)?;
                }
            }
        }
        Content::Object(Object::Atlas { fallback, .. } | Object::Player { fallback, .. }) => {
            if let Some(fallback) = fallback {
                validate_component_syntax(fallback)?;
            }
        }
        Content::Resolvable(resolvable) => validate_resolvable_syntax(resolvable)?,
        Content::Text { .. } | Content::Keybind { .. } | Content::Custom(_) => {}
    }
    for child in &component.children {
        validate_component_syntax(child)?;
    }
    match &component.interactions.hover {
        Some(
            HoverEvent::ShowText { value }
            | HoverEvent::ShowEntity {
                name: Some(value), ..
            },
        ) => validate_component_syntax(value)?,
        Some(HoverEvent::ShowItem { .. } | HoverEvent::ShowEntity { name: None, .. }) | None => {}
    }
    Ok(())
}

fn validate_resolvable_syntax(resolvable: &Resolvable) -> Result<(), String> {
    match resolvable {
        Resolvable::Scoreboard { .. } => {}
        Resolvable::Entity {
            selector,
            separator,
        } => {
            parse_entity_selector_text(selector).map_err(|error| error.to_string())?;
            if let Some(separator) = separator {
                validate_component_syntax(separator)?;
            }
        }
        Resolvable::NBT {
            path,
            separator,
            source,
            ..
        } => {
            parse_nbt_path(path).map_err(|error| error.to_string())?;
            match source {
                NbtSource::Entity(selector) => {
                    parse_entity_selector_text(selector).map_err(|error| error.to_string())?;
                }
                NbtSource::Block(coordinates) => {
                    parse_block_coordinates(coordinates).map_err(|error| error.to_string())?;
                }
                NbtSource::Storage(identifier) => {
                    parse_resource_identifier(identifier).map_err(str::to_owned)?;
                }
            }
            if let Some(separator) = separator {
                validate_component_syntax(separator)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use simdnbt::owned::{NbtCompound, NbtList};
    use text_components::{
        Modifier as _,
        content::{Content, NbtSource, Resolvable},
        format::Color,
    };

    use steel_utils::text::DisplayResolutor;

    use super::{
        CommandSyntaxError, CommandTextResolutionSource, CommandTextResolver, NbtTag, TextComponent,
    };

    #[derive(Default)]
    struct TestSource {
        display_names: BTreeMap<String, Vec<TextComponent>>,
        score_names: BTreeMap<String, Option<Vec<String>>>,
        scores: BTreeMap<(String, String), i32>,
        nbt: BTreeMap<String, Vec<NbtTag>>,
    }

    impl CommandTextResolutionSource for TestSource {
        fn selector_display_names(
            &self,
            selector: &str,
        ) -> Result<Vec<TextComponent>, CommandSyntaxError> {
            Ok(self
                .display_names
                .get(selector)
                .cloned()
                .unwrap_or_default())
        }

        fn score_selector_names(
            &self,
            selector: &str,
        ) -> Result<Option<Vec<String>>, CommandSyntaxError> {
            Ok(self.score_names.get(selector).cloned().flatten())
        }

        fn score(&self, holder: &str, objective: &str) -> Result<Option<i32>, CommandSyntaxError> {
            Ok(self
                .scores
                .get(&(holder.to_owned(), objective.to_owned()))
                .copied())
        }

        fn nbt_source(&self, source: &NbtSource) -> Result<Vec<NbtTag>, CommandSyntaxError> {
            let NbtSource::Storage(identifier) = source else {
                return Ok(Vec::new());
            };
            Ok(self
                .nbt
                .get(identifier.as_ref())
                .cloned()
                .unwrap_or_default())
        }
    }

    fn resolver<'a>(
        source: &'a TestSource,
        default_scoreboard_name: Option<&str>,
    ) -> CommandTextResolver<'a, TestSource> {
        CommandTextResolver {
            source,
            default_scoreboard_name: default_scoreboard_name.map(str::to_owned),
        }
    }

    #[test]
    fn selectors_use_resolved_separators_and_preserve_display_components() {
        let mut source = TestSource::default();
        source.display_names.insert(
            "@a".to_owned(),
            vec![
                TextComponent::plain("Alex").color(Color::Red),
                TextComponent::plain("Steve"),
            ],
        );
        let component =
            TextComponent::entity("@a", Some(TextComponent::plain(" | ").color(Color::Gold)));
        let Ok(resolved) = component.try_resolve(&resolver(&source, None)) else {
            panic!("selector component should resolve");
        };

        assert_eq!(resolved.to_plain(&DisplayResolutor), "Alex | Steve");
        assert_eq!(resolved.children[0].format.color, Some(Color::Red));
        assert_eq!(resolved.children[1].format.color, Some(Color::Gold));
    }

    #[test]
    fn score_wildcard_resolves_separately_for_each_recipient() {
        let mut source = TestSource::default();
        source
            .scores
            .insert(("Alex".to_owned(), "points".to_owned()), 3);
        source
            .scores
            .insert(("Steve".to_owned(), "points".to_owned()), 8);
        let component = TextComponent::scoreboard("*", "points");

        let Ok(alex) = component.try_resolve(&resolver(&source, Some("Alex"))) else {
            panic!("Alex's score should resolve");
        };
        let Ok(steve) = component.try_resolve(&resolver(&source, Some("Steve"))) else {
            panic!("Steve's score should resolve");
        };
        assert_eq!(alex.to_plain(&DisplayResolutor), "3");
        assert_eq!(steve.to_plain(&DisplayResolutor), "8");
    }

    #[test]
    fn score_selectors_require_at_most_one_entity_and_fall_back_when_empty() {
        let mut source = TestSource::default();
        source.score_names.insert(
            "@a".to_owned(),
            Some(vec!["Alex".to_owned(), "Steve".to_owned()]),
        );
        source
            .scores
            .insert(("@s".to_owned(), "points".to_owned()), 5);

        assert!(
            TextComponent::scoreboard("@a", "points")
                .try_resolve(&resolver(&source, None))
                .is_err()
        );
        let Ok(empty_selector) =
            TextComponent::scoreboard("@s", "points").try_resolve(&resolver(&source, None))
        else {
            panic!("empty selector should fall back to its raw holder name");
        };
        assert_eq!(empty_selector.to_plain(&DisplayResolutor), "5");
    }

    #[test]
    fn nbt_resolution_selects_paths_and_applies_plain_rendering() {
        let mut first = NbtCompound::new();
        first.insert("values", NbtList::Int(vec![1, 2]));
        let mut source = TestSource::default();
        source
            .nbt
            .insert("minecraft:test".to_owned(), vec![NbtTag::Compound(first)]);
        let component = TextComponent {
            content: Content::Resolvable(Resolvable::NBT {
                path: "values[]".into(),
                interpret: false,
                plain: true,
                separator: Some(Box::new(TextComponent::plain(" / "))),
                source: NbtSource::storage("minecraft:test"),
            }),
            ..Default::default()
        };
        let Ok(resolved) = component.try_resolve(&resolver(&source, None)) else {
            panic!("NBT component should resolve");
        };

        assert_eq!(resolved.to_plain(&DisplayResolutor), "1 / 2");
        assert!(
            resolved
                .children
                .iter()
                .all(|child| child.format.color.is_none())
        );
    }

    #[test]
    fn interpreted_nbt_resolves_nested_scores_at_the_same_depth() {
        let mut source = TestSource::default();
        source
            .scores
            .insert(("Alex".to_owned(), "points".to_owned()), 12);
        let mut component = NbtCompound::new();
        let mut score = NbtCompound::new();
        score.insert("name", "Alex");
        score.insert("objective", "points");
        component.insert("score", score);
        let mut root = NbtCompound::new();
        root.insert("message", component);
        source
            .nbt
            .insert("minecraft:test".to_owned(), vec![NbtTag::Compound(root)]);
        let component =
            TextComponent::nbt("message", NbtSource::storage("minecraft:test"), true, None);
        let Ok(resolved) = component.try_resolve(&resolver(&source, None)) else {
            panic!("interpreted NBT component should resolve");
        };

        assert_eq!(resolved.to_plain(&DisplayResolutor), "12");
    }

    #[test]
    fn interpreted_nbt_skips_components_with_invalid_compiled_strings() {
        let mut invalid = NbtCompound::new();
        invalid.insert("selector", "@e[");
        let mut valid = NbtCompound::new();
        valid.insert("text", "valid");
        let mut root = NbtCompound::new();
        root.insert("messages", NbtList::Compound(vec![invalid, valid]));
        let mut source = TestSource::default();
        source
            .nbt
            .insert("minecraft:test".to_owned(), vec![NbtTag::Compound(root)]);
        let component = TextComponent::nbt(
            "messages[]",
            NbtSource::storage("minecraft:test"),
            true,
            None,
        );
        let Ok(resolved) = component.try_resolve(&resolver(&source, None)) else {
            panic!("valid interpreted NBT components should still resolve");
        };

        assert_eq!(resolved.to_plain(&DisplayResolutor), "valid");
    }
}
