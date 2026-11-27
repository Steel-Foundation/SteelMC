use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use text_components::TextComponent;
use uuid::Uuid;

use crate::{UuidExt, translations};

use super::{
    SnbtErrorKind, SnbtNumberType, parse_snbt, parse_snbt_argument, parse_snbt_compound,
    to_canonical_snbt,
};

fn compound_tag(input: &str) -> NbtCompound {
    parse_snbt_compound(input).expect("compound parses")
}

#[test]
fn canonical_writer_matches_vanilla_snbt_shapes() {
    let mut nested = NbtCompound::new();
    nested.insert("true", "a\"b'c\\d\n");
    nested.insert("alpha", f64::MIN_POSITIVE);
    let mut compound = NbtCompound::new();
    compound.insert("z", NbtList::Int(vec![1, 2]));
    compound.insert("a", nested);

    assert_eq!(
        to_canonical_snbt(&NbtTag::Compound(compound)),
        Some("{a:{alpha:2.2250738585072014E-308d,\"true\":'a\"b\\'c\\\\d\\n'},z:[1,2]}".to_owned())
    );
}

#[test]
fn canonical_writer_uses_semantic_heterogeneous_list_values() {
    let list = NbtTag::List(NbtList::from(vec![
        NbtTag::Int(7),
        NbtTag::String("value".into()),
    ]));

    assert_eq!(to_canonical_snbt(&list), Some("[7,\"value\"]".to_owned()));
}

#[test]
fn canonical_writer_preserves_wrapper_shaped_compound_values() {
    let mut compound = NbtCompound::new();
    compound.insert("", 7);
    let list = NbtTag::List(NbtList::from(vec![
        NbtTag::Int(1),
        NbtTag::Compound(compound),
    ]));

    assert_eq!(to_canonical_snbt(&list), Some("[1,{\"\":7}]".to_owned()));
}

#[test]
fn canonical_writer_uses_java_floating_point_decimals() {
    for (tag, expected) in [
        (NbtTag::Float(f32::from_bits(1)), "1.4E-45f"),
        (NbtTag::Float(f32::MIN_POSITIVE), "1.1754944E-38f"),
        (NbtTag::Float(9_999_999.0), "9999999.0f"),
        (NbtTag::Float(10_000_000.0), "1.0E7f"),
        (NbtTag::Double(f64::from_bits(1)), "4.9E-324d"),
        (NbtTag::Double(0.001), "0.001d"),
        (NbtTag::Double(0.0001), "1.0E-4d"),
        (NbtTag::Double(f64::INFINITY), "Infinityd"),
        (NbtTag::Double(f64::NEG_INFINITY), "-Infinityd"),
        (NbtTag::Double(f64::NAN), concat!("NaN", "d")),
    ] {
        assert_eq!(to_canonical_snbt(&tag).as_deref(), Some(expected));
    }
}

#[test]
fn parses_compounds_lists_and_trailing_commas() {
    let compound = compound_tag("{name:'steel', flags:[true,false,], nested:{value:1b,},}");

    assert_eq!(
        compound
            .string("name")
            .map(|value| value.to_str().into_owned()),
        Some("steel".to_owned())
    );
    assert_eq!(
        compound.get("flags"),
        Some(&NbtTag::List(NbtList::Byte(vec![1, 0])))
    );
    assert_eq!(
        compound
            .compound("nested")
            .and_then(|nested| nested.byte("value")),
        Some(1)
    );
}

#[test]
fn parses_boolean_literals_case_insensitively() {
    let compound = compound_tag("{upper:TRUE,mixed:FaLsE}");

    assert_eq!(compound.byte("upper"), Some(1));
    assert_eq!(compound.byte("mixed"), Some(0));
}

#[test]
fn duplicate_compound_keys_keep_last_value() {
    let compound = compound_tag("{value:1,value:2}");

    assert_eq!(compound.int("value"), Some(2));
    assert_eq!(compound.len(), 1);
}

#[test]
fn parses_integer_widths_and_unsigned_literals() {
    let compound = compound_tag("{a:1b,b:2s,c:3,d:4l,e:0xFFuB,f:0b1010,g:1_000}");

    assert_eq!(compound.byte("a"), Some(1));
    assert_eq!(compound.short("b"), Some(2));
    assert_eq!(compound.int("c"), Some(3));
    assert_eq!(compound.long("d"), Some(4));
    assert_eq!(compound.byte("e"), Some(-1));
    assert_eq!(compound.int("f"), Some(10));
    assert_eq!(compound.int("g"), Some(1000));
}

#[test]
fn hexadecimal_number_runs_are_greedy_before_suffixes() {
    let compound = compound_tag("{first:0xAB,second:0x1B}");

    assert_eq!(compound.int("first"), Some(0xAB));
    assert_eq!(compound.int("second"), Some(0x1B));
}

#[test]
fn zero_with_a_byte_suffix_is_not_a_binary_prefix() {
    assert_eq!(parse_snbt("0b").expect("byte zero parses"), NbtTag::Byte(0));
    assert_eq!(parse_snbt("0B").expect("byte zero parses"), NbtTag::Byte(0));
}

#[test]
fn negative_radix_literals_require_explicit_signed_suffixes() {
    assert_eq!(
        parse_snbt("-0x1sI").expect("explicitly signed hex literal parses"),
        NbtTag::Int(-1)
    );
    assert_eq!(
        parse_snbt("-0b1sB").expect("explicitly signed binary literal parses"),
        NbtTag::Byte(-1)
    );

    for literal in ["-0x1", "-0b1", "-0x1i", "-0b1B", "-0x1uI", "-0b1uB"] {
        assert!(parse_snbt(literal).is_err(), "{literal} should not parse");
    }
}

#[test]
fn number_runs_allow_repeated_interior_underscores() {
    let compound =
        compound_tag("{decimal:1__2,binary:0b1__0,hex:0xA__B,float:1__2.3__4,exponent:1e1__2}");

    assert_eq!(compound.int("decimal"), Some(12));
    assert_eq!(compound.int("binary"), Some(2));
    assert_eq!(compound.int("hex"), Some(0xAB));
    assert_eq!(compound.double("float"), Some(12.34));
    assert_eq!(compound.double("exponent"), Some(1e12));
}

#[test]
fn parses_floating_point_literals() {
    let compound = compound_tag("{float:1.5f,double:2.5d,exponent:1e2,underscored:1_2.5}");

    assert_eq!(compound.float("float"), Some(1.5));
    assert_eq!(compound.double("double"), Some(2.5));
    assert_eq!(compound.double("exponent"), Some(100.0));
    assert_eq!(compound.double("underscored"), Some(12.5));
}

#[test]
fn rejects_underscores_at_number_run_boundaries() {
    for literal in [
        "+_1", "1_", "0x_1", "0x1_", "0b_1", "0b1_", "1_.0", "1._0", "1_e2", "1e_2", "1e+_2",
        "1.0_", "1e2_",
    ] {
        let input = format!("{{value:{literal}}}");
        assert!(
            parse_snbt_compound(&input).is_err(),
            "{literal} should not parse"
        );
    }
}

#[test]
fn parses_typed_arrays() {
    let compound = compound_tag("{bytes:[B;1b,255uB],ints:[I;1,2b,3s],longs:[L;1,2i,3l]}");

    assert_eq!(compound.byte_array("bytes"), Some([1, 255].as_slice()));
    assert_eq!(compound.int_array("ints"), Some([1, 2, 3].as_slice()));
    assert_eq!(compound.long_array("longs"), Some([1, 2, 3].as_slice()));
}

#[test]
fn parses_builtins() {
    let uuid =
        Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").expect("uuid literal parses");
    let compound = compound_tag(
        "{enabled:bool(1),id:uuid('123e4567-e89b-12d3-a456-426614174000'),compact:uuid('123e4567e89b12d3a456426614174000')}",
    );

    assert_eq!(compound.byte("enabled"), Some(1));
    assert_eq!(
        compound.int_array("id"),
        Some(uuid.to_int_array().as_slice())
    );
    assert_eq!(
        compound.int_array("compact"),
        Some(uuid.to_int_array().as_slice())
    );
}

#[test]
fn builtin_operation_lookup_uses_the_actual_argument_count() {
    for (input, name, argument_count) in [
        ("unknown()", "unknown", 0),
        ("unknown(1)", "unknown", 1),
        ("bool()", "bool", 0),
        ("bool(1,2)", "bool", 2),
    ] {
        let error = parse_snbt(input).expect_err("operation arity should not match");

        assert_eq!(error.cursor(), input.len(), "{input}");
        assert_eq!(
            error.kind(),
            &SnbtErrorKind::UnknownOperation {
                name: name.to_owned(),
                argument_count,
            },
            "{input}"
        );
        assert_eq!(
            error.component(),
            translations::SNBT_PARSER_NO_SUCH_OPERATION
                .message([format!("{name}/{argument_count}")])
                .component(),
            "{input}"
        );
    }

    assert_eq!(
        parse_snbt("bool(1,)").expect("a trailing argument separator is valid"),
        NbtTag::Byte(1)
    );
}

#[test]
fn parses_string_escapes() {
    let compound = compound_tag(r#"{text:"\x41\u0042\U00000043\N{LATIN CAPITAL LETTER D}"}"#);

    assert_eq!(
        compound
            .string("text")
            .map(|value| value.to_str().into_owned()),
        Some("ABCD".to_owned())
    );
}

#[test]
fn argument_parser_does_not_consume_trailing_whitespace() {
    let (tag, cursor) = parse_snbt_argument("{value:1} run").expect("tag parses");

    assert!(matches!(tag, NbtTag::Compound(_)));
    assert_eq!(cursor, "{value:1}".len());
}

#[test]
fn full_parser_rejects_trailing_data() {
    let error = parse_snbt("{value:1} trailing").expect_err("trailing data should fail");

    assert_eq!(error.cursor(), "{value:1} ".len());
    assert_eq!(error.kind(), &SnbtErrorKind::TrailingData);
    assert_eq!(
        error.component(),
        TextComponent::from(&translations::ARGUMENT_NBT_TRAILING)
    );
}

#[test]
fn errors_preserve_semantic_kinds_and_translation_arguments() {
    let expected_value = parse_snbt("{value:}").expect_err("missing value should fail");
    assert_eq!(expected_value.kind(), &SnbtErrorKind::ExpectedValue);
    assert_eq!(
        expected_value.component(),
        TextComponent::from(&translations::SNBT_PARSER_EXPECTED_UNQUOTED_STRING)
    );

    let expected_key = parse_snbt("{:1}").expect_err("missing key should fail");
    assert_eq!(expected_key.kind(), &SnbtErrorKind::ExpectedKey);
    assert_eq!(
        expected_key.component(),
        translations::ARGUMENT_LITERAL_INCORRECT
            .message(["\""])
            .component()
    );

    let expected_number = parse_snbt("[B;,]").expect_err("missing typed-array number should fail");
    assert_eq!(expected_number.kind(), &SnbtErrorKind::ExpectedNumber);
    assert_eq!(
        expected_number.component(),
        translations::ARGUMENT_LITERAL_INCORRECT
            .message(["+"])
            .component()
    );

    let invalid_underscore =
        parse_snbt("0b1_").expect_err("trailing binary underscore should fail");
    assert_eq!(invalid_underscore.kind(), &SnbtErrorKind::InvalidUnderscore);
    assert_eq!(
        invalid_underscore.component(),
        TextComponent::from(&translations::SNBT_PARSER_UNDERSCORE_NOT_ALLOWED)
    );

    let invalid_uuid = parse_snbt("uuid('invalid')").expect_err("invalid UUID should fail");
    assert_eq!(invalid_uuid.kind(), &SnbtErrorKind::ExpectedStringUuid);
    assert_eq!(
        invalid_uuid.component(),
        TextComponent::from(&translations::SNBT_PARSER_EXPECTED_STRING_UUID)
    );

    let unknown_operation = parse_snbt("unknown(1)").expect_err("unknown operation should fail");
    assert_eq!(
        unknown_operation.kind(),
        &SnbtErrorKind::UnknownOperation {
            name: "unknown".to_owned(),
            argument_count: 1,
        }
    );
    assert_eq!(
        unknown_operation.component(),
        translations::SNBT_PARSER_NO_SUCH_OPERATION
            .message(["unknown/1"])
            .component()
    );
}

#[test]
fn numeric_syntax_errors_preserve_specific_kinds_and_cursors() {
    for (input, cursor, kind, component) in [
        (
            "+",
            1,
            SnbtErrorKind::ExpectedDecimalNumeral,
            TextComponent::from(&translations::SNBT_PARSER_EXPECTED_DECIMAL_NUMERAL),
        ),
        (
            ".",
            1,
            SnbtErrorKind::ExpectedDecimalNumeral,
            TextComponent::from(&translations::SNBT_PARSER_EXPECTED_DECIMAL_NUMERAL),
        ),
        (
            "0x",
            2,
            SnbtErrorKind::ExpectedHexNumeral,
            TextComponent::from(&translations::SNBT_PARSER_EXPECTED_HEX_NUMERAL),
        ),
    ] {
        let error = parse_snbt(input).expect_err("incomplete number should fail");

        assert_eq!(error.cursor(), cursor, "{input}");
        assert_eq!(error.kind(), &kind, "{input}");
        assert_eq!(error.component(), component, "{input}");
    }

    let signed_nonnumeric = parse_snbt("-x").expect_err("signed string should fail");
    assert_eq!(signed_nonnumeric.cursor(), 1);
    assert_eq!(
        signed_nonnumeric.kind(),
        &SnbtErrorKind::ExpectedDecimalNumeral
    );

    let out_of_range = parse_snbt("128b").expect_err("out-of-range byte should fail");
    assert_eq!(out_of_range.cursor(), "128b".len());
    assert_eq!(
        out_of_range.kind(),
        &SnbtErrorKind::NumberOutOfRange {
            number_type: SnbtNumberType::Byte,
            unsigned: false,
        }
    );
}

#[test]
fn argument_parser_stops_after_a_complete_number() {
    let (tag, cursor) = parse_snbt_argument("1z").expect("integer prefix parses");

    assert_eq!(tag, NbtTag::Int(1));
    assert_eq!(cursor, 1);
}

#[test]
fn errors_match_vanilla_alternative_selection_and_cursors() {
    for (input, cursor, kind, expected_literal) in [
        ("{a:1", 4, SnbtErrorKind::ExpectedSymbol('.'), "."),
        ("{a:true", 7, SnbtErrorKind::ExpectedSymbol('('), "("),
        (r#"{a:"x""#, 6, SnbtErrorKind::ExpectedSymbol(','), ","),
        (r#""\q""#, 2, SnbtErrorKind::InvalidEscape('q'), "b"),
        (r#""\N""#, 3, SnbtErrorKind::ExpectedCharacterName, "{"),
        (r#""\N{ABC""#, 7, SnbtErrorKind::UnclosedCharacterName, "}"),
    ] {
        let error = parse_snbt(input).expect_err("input should not parse");

        assert_eq!(error.cursor(), cursor, "{input}");
        assert_eq!(error.kind(), &kind, "{input}");
        assert_eq!(
            error.component(),
            translations::ARGUMENT_LITERAL_INCORRECT
                .message([expected_literal])
                .component(),
            "{input}"
        );
    }

    let unclosed_string = parse_snbt(r#""abc"#).expect_err("unclosed string should fail");
    assert_eq!(unclosed_string.cursor(), 4);
    assert_eq!(unclosed_string.kind(), &SnbtErrorKind::UnclosedQuotedString);
    assert_eq!(
        unclosed_string.component(),
        TextComponent::from(&translations::SNBT_PARSER_INVALID_STRING_CONTENTS)
    );
}
