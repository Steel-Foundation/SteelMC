use super::*;

#[test]
fn score_holder_argument_retains_deferred_names_uuids_selectors_and_wildcards() {
    let single = resource_dispatcher(SteelArgumentType::score_holder());

    let parse = single.parse("resource Player", TestSource::new());
    let Ok(chain) = single.context_chain(parse) else {
        panic!("direct score holder names should parse");
    };
    assert!(matches!(
        chain.top_context().score_holder_argument("value"),
        Ok(ScoreHolderArgument::Name(name)) if name.as_ref() == "Player"
    ));

    let raw_uuid = "00000000-0000-0000-0000-000000000001";
    let uuid_command = format!("resource {raw_uuid}");
    let parse = single.parse(&uuid_command, TestSource::new());
    let Ok(chain) = single.context_chain(parse) else {
        panic!("UUID score holders should parse");
    };
    assert!(matches!(
        chain.top_context().score_holder_argument("value"),
        Ok(ScoreHolderArgument::Uuid { raw, .. }) if raw.as_ref() == raw_uuid
    ));

    let parse = single.parse("resource @s", TestSource::new());
    let Ok(chain) = single.context_chain(parse) else {
        panic!("single-result entity selectors should parse as score holders");
    };
    assert!(matches!(
        chain.top_context().score_holder_argument("value"),
        Ok(ScoreHolderArgument::Selector(_))
    ));
    let parse = single.parse("resource @a", TestSource::new());
    assert!(single.context_chain(parse).is_err());

    let multiple = resource_dispatcher(SteelArgumentType::score_holders());
    let parse = multiple.parse("resource *", TestSource::new());
    let Ok(chain) = multiple.context_chain(parse) else {
        panic!("wildcard score holders should parse");
    };
    assert!(matches!(
        chain.top_context().score_holder_argument("value"),
        Ok(ScoreHolderArgument::Wildcard)
    ));

    let parse = single.parse("resource S", TestSource::new());
    let Ok(suggestions) = single.completion_suggestions(&parse) else {
        panic!("score holder suggestions should build");
    };
    assert!(
        suggestions
            .list()
            .iter()
            .any(|suggestion| suggestion.text() == "Steve")
    );
}

#[test]
fn objective_and_integer_range_arguments_retain_vanilla_values() {
    let objective = resource_dispatcher(SteelArgumentType::objective());
    let parse = objective.parse("resource kills", TestSource::new());
    let Ok(chain) = objective.context_chain(parse) else {
        panic!("objective names should parse");
    };
    assert_eq!(chain.top_context().objective_name("value"), Ok("kills"));
    let parse = objective.parse("resource k", TestSource::new());
    let Ok(suggestions) = objective.completion_suggestions(&parse) else {
        panic!("objective suggestions should build");
    };
    assert_eq!(
        suggestions
            .list()
            .iter()
            .map(Suggestion::text)
            .collect::<Vec<_>>(),
        ["kills"]
    );

    let range = resource_dispatcher(SteelArgumentType::int_range());
    for (input, matches, misses) in [
        ("resource 5", 5, 4),
        ("resource -5..10", 0, 11),
        ("resource ..10", i32::MIN, 11),
        ("resource -5..", i32::MAX, -6),
    ] {
        let parse = range.parse(input, TestSource::new());
        let Ok(chain) = range.context_chain(parse) else {
            panic!("{input} should parse as an integer range");
        };
        let Ok(value) = chain.top_context().int_range("value") else {
            panic!("integer range should be retained");
        };
        assert!(value.matches(matches));
        assert!(!value.matches(misses));
    }

    for input in ["resource ..", "resource 5..2", "resource 1.5"] {
        let parse = range.parse(input, TestSource::new());
        assert!(
            range.context_chain(parse).is_err(),
            "{input} should reject an invalid integer range"
        );
    }
}

#[test]
fn block_predicate_argument_parses_blocks_tags_properties_and_nbt() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::block_predicate());

    let parse = dispatcher.parse(
        "resource oak_log[axis=x]{custom:{value:3}}",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("concrete block predicate should parse");
    };
    let Ok(predicate) = chain.top_context().block_predicate("value") else {
        panic!("block predicate should be retained");
    };
    let Some(oak_x) = steel_registry::REGISTRY
        .blocks
        .state_id_from_block_defaulted_properties(&vanilla_blocks::OAK_LOG, [("axis", "x")])
    else {
        panic!("oak log x state should exist");
    };
    assert!(predicate.matches_state(oak_x));
    assert!(!predicate.matches_state(vanilla_blocks::OAK_LOG.default_state()));
    let Some(nbt) = predicate.nbt() else {
        panic!("block predicate NBT should be retained");
    };
    assert_eq!(
        nbt.compound("custom")
            .and_then(|custom| custom.int("value")),
        Some(3)
    );

    let parse = dispatcher.parse(
        "resource #c:natural_logs/overworld[axis=y]",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("block tag predicate should parse");
    };
    let Ok(BlockPredicate::Tag { .. }) = chain.top_context().block_predicate("value") else {
        panic!("block tag predicate should be retained");
    };
    let Ok(predicate) = chain.top_context().block_predicate("value") else {
        panic!("block tag predicate should be retained");
    };
    assert!(predicate.matches_state(vanilla_blocks::OAK_LOG.default_state()));
    assert!(!predicate.matches_state(vanilla_blocks::STONE.default_state()));
}

#[test]
fn block_state_argument_parses_defaulted_properties_and_nbt_without_tags() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::block_state());
    let parse = dispatcher.parse(
        "resource oak_log[axis=x]{custom:{value:3}}",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("concrete block state should parse");
    };
    let Ok(input) = chain.top_context().block_input("value") else {
        panic!("block input should be retained");
    };
    let Some(expected) = steel_registry::REGISTRY
        .blocks
        .state_id_from_block_defaulted_properties(&vanilla_blocks::OAK_LOG, [("axis", "x")])
    else {
        panic!("oak log x state should exist");
    };
    assert_eq!(input.state(), expected);
    assert_eq!(
        input
            .nbt()
            .and_then(|nbt| nbt.compound("custom"))
            .and_then(|custom| custom.int("value")),
        Some(3)
    );

    for input in [
        "resource #c:natural_logs/overworld",
        "resource oak_log[missing=value]",
        "resource oak_log[axis=missing]",
    ] {
        let parse = dispatcher.parse(input, TestSource::new());
        assert!(
            dispatcher.context_chain(parse).is_err(),
            "{input} should reject an invalid concrete block state"
        );
    }
}

#[test]
fn block_arguments_use_vanilla_diagnostics() {
    init_vanilla_registry();
    let block_state = resource_dispatcher(SteelArgumentType::block_state());
    let block_predicate = resource_dispatcher(SteelArgumentType::block_predicate());

    for (dispatcher, input, translation, cursor) in [
        (
            &block_state,
            "resource missing_block",
            "argument.block.id.invalid",
            "resource ".len(),
        ),
        (
            &block_state,
            "resource oak_log[missing=value]",
            "argument.block.property.unknown",
            "resource oak_log[".len(),
        ),
        (
            &block_state,
            "resource oak_log[axis=x,axis=y]",
            "argument.block.property.duplicate",
            "resource oak_log[axis=x,".len(),
        ),
        (
            &block_state,
            "resource oak_log[axis=missing]",
            "argument.block.property.invalid",
            "resource oak_log[axis=".len(),
        ),
        (
            &block_state,
            "resource oak_log[axis]",
            "argument.block.property.novalue",
            "resource oak_log[axis".len(),
        ),
        (
            &block_state,
            "resource oak_log[axis=x",
            "argument.block.property.unclosed",
            "resource oak_log[axis=x".len(),
        ),
        (
            &block_state,
            "resource #minecraft:logs",
            "argument.block.tag.disallowed",
            "resource ".len(),
        ),
        (
            &block_predicate,
            "resource #missing",
            "arguments.block.tag.unknown",
            "resource ".len(),
        ),
    ] {
        let parse = dispatcher.parse(input, TestSource::new());
        let Err(error) = dispatcher.context_chain(parse) else {
            panic!("{input} should fail");
        };
        assert_eq!(error.cursor(), Some(cursor), "{input}");
        let CommandSyntaxErrorKind::Dynamic(component) = error.kind() else {
            panic!("{input} should use a translated dynamic error");
        };
        assert!(
            matches!(&component.content, Content::Translate(message) if message.key == translation),
            "{input} used {component:?} instead of {translation}"
        );
    }
}

#[test]
fn block_arguments_use_vanilla_stateful_suggestions() {
    init_vanilla_registry();
    let block_state = resource_dispatcher(SteelArgumentType::block_state());
    let block_predicate = resource_dispatcher(SteelArgumentType::block_predicate());

    for (input, expected) in [
        ("resource oak_log[ax", "oak_log[axis="),
        ("resource oak_log[axis=", "oak_log[axis=x"),
        ("resource oak_log[axis=x", "oak_log[axis=x]"),
        ("resource furnace", "furnace{"),
    ] {
        let parse = block_state.parse(input, TestSource::new());
        let Ok(suggestions) = block_state.completion_suggestions(&parse) else {
            panic!("{input} suggestions should build");
        };
        assert!(
            suggestions
                .list()
                .iter()
                .any(|suggestion| suggestion.text() == expected),
            "{input} should suggest {expected}; got {:?}",
            suggestions
                .list()
                .iter()
                .map(Suggestion::text)
                .collect::<Vec<_>>()
        );
    }

    let input = "resource #c:natural_logs/overworld[ax";
    let parse = block_predicate.parse(input, TestSource::new());
    let Ok(suggestions) = block_predicate.completion_suggestions(&parse) else {
        panic!("tag property suggestions should build");
    };
    assert!(
        suggestions
            .list()
            .iter()
            .any(|suggestion| { suggestion.text() == "#c:natural_logs/overworld[axis=" })
    );

    let input = "resource #c:natural_logs/overworld[unknown=value";
    let parse = block_predicate.parse(input, TestSource::new());
    let Ok(suggestions) = block_predicate.completion_suggestions(&parse) else {
        panic!("vague tag property suggestions should build");
    };
    assert!(
        suggestions
            .list()
            .iter()
            .any(|suggestion| { suggestion.text() == "#c:natural_logs/overworld[unknown=value]" })
    );

    for input in [
        "resource oak_log[axis=x,axis",
        "resource oak_log[axis=x,axis=",
        "resource oak_log[axis=x,",
    ] {
        let parse = block_state.parse(input, TestSource::new());
        let Ok(suggestions) = block_state.completion_suggestions(&parse) else {
            panic!("{input} suggestions should build");
        };
        assert!(
            suggestions.list().is_empty(),
            "{input} should have no completions; got {:?}",
            suggestions
                .list()
                .iter()
                .map(Suggestion::text)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn block_predicate_argument_validates_concrete_properties_but_defers_tag_properties() {
    init_vanilla_registry();
    let dispatcher = resource_dispatcher(SteelArgumentType::block_predicate());

    for input in [
        "resource oak_log[missing=value]",
        "resource oak_log[axis=missing]",
        "resource oak_log[axis=x,axis=y]",
        "resource #missing",
    ] {
        let parse = dispatcher.parse(input, TestSource::new());
        assert!(
            dispatcher.context_chain(parse).is_err(),
            "{input} should reject an invalid block predicate"
        );
    }

    let parse = dispatcher.parse(
        "resource #c:natural_logs/overworld[missing=value]",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("vague tag properties should parse");
    };
    let Ok(predicate) = chain.top_context().block_predicate("value") else {
        panic!("block tag predicate should be retained");
    };
    assert!(!predicate.matches_state(vanilla_blocks::OAK_LOG.default_state()));
}

#[test]
fn nbt_path_argument_retains_vanilla_path_nodes() {
    let dispatcher = resource_dispatcher(SteelArgumentType::nbt_path());
    let parse = dispatcher.parse(
        "resource items[{id:\"minecraft:stone\"}].Count",
        TestSource::new(),
    );
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("NBT path should parse");
    };
    let Ok(path) = chain.top_context().nbt_path("value") else {
        panic!("NBT path should be retained");
    };

    assert_eq!(path.as_str(), "items[{id:\"minecraft:stone\"}].Count");
}

#[test]
fn swizzle_argument_retains_unique_axes_and_rejects_duplicates() {
    let dispatcher = resource_dispatcher(SteelArgumentType::swizzle());
    let parse = dispatcher.parse("resource zx", TestSource::new());
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("unique swizzle axes should parse");
    };
    let Ok(axes) = chain.top_context().swizzle("value") else {
        panic!("swizzle axes should be retained");
    };

    assert!(axes.x());
    assert!(!axes.y());
    assert!(axes.z());
    assert_eq!(
        axes.align(DVec3::new(1.9, 2.9, -1.1)),
        DVec3::new(1.0, 2.9, -2.0)
    );

    for input in ["resource xx", "resource q"] {
        let parse = dispatcher.parse(input, TestSource::new());
        assert!(
            dispatcher.context_chain(parse).is_err(),
            "{input} should reject an invalid swizzle"
        );
    }
}
