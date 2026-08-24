#![expect(
    clippy::unwrap_used,
    reason = "test assertions: unwrap panics on parse failures"
)]

use super::*;
use crate::random::{Random, legacy_random::LegacyRandom};

#[test]
fn vertical_anchor_resolve() {
    assert_eq!(VerticalAnchor::Absolute(42).resolve_y(-64, 384), 42);
    assert_eq!(VerticalAnchor::AboveBottom(8).resolve_y(-64, 384), -56);
    assert_eq!(VerticalAnchor::BelowTop(1).resolve_y(0, 128), 126);
}

#[test]
fn vertical_anchor_deserialize() {
    let a: VerticalAnchor = serde_json::from_str(r#"{"absolute": 180}"#).unwrap();
    assert_eq!(a, VerticalAnchor::Absolute(180));
    let b: VerticalAnchor = serde_json::from_str(r#"{"above_bottom": 8}"#).unwrap();
    assert_eq!(b, VerticalAnchor::AboveBottom(8));
    let c: VerticalAnchor = serde_json::from_str(r#"{"below_top": 1}"#).unwrap();
    assert_eq!(c, VerticalAnchor::BelowTop(1));
    assert!(serde_json::from_str::<VerticalAnchor>(r"{}").is_err());
    assert!(
        serde_json::from_str::<VerticalAnchor>(r#"{"absolute": 1, "above_bottom": 2}"#).is_err()
    );
}

#[test]
fn height_provider_deserialize_shortcut() {
    // A bare VerticalAnchor is a ConstantHeight.
    let hp: HeightProvider = serde_json::from_str(r#"{"absolute": 180}"#).unwrap();
    match hp {
        HeightProvider::Constant(VerticalAnchor::Absolute(180)) => (),
        other => panic!("expected Constant(Absolute(180)), got {other:?}"),
    }
}

#[test]
fn height_provider_uniform_from_carver_json() {
    let hp: HeightProvider = serde_json::from_str(
        r#"{
                "type": "minecraft:uniform",
                "max_inclusive": {"absolute": 180},
                "min_inclusive": {"above_bottom": 8}
            }"#,
    )
    .unwrap();
    match hp {
        HeightProvider::Uniform {
            min_inclusive,
            max_inclusive,
        } => {
            assert_eq!(min_inclusive, VerticalAnchor::AboveBottom(8));
            assert_eq!(max_inclusive, VerticalAnchor::Absolute(180));
        }
        other => panic!("expected Uniform, got {other:?}"),
    }
}

#[test]
fn float_provider_bare_float() {
    let fp: FloatProvider = serde_json::from_str("3.0").unwrap();
    match fp {
        FloatProvider::Constant(v) => assert!((v - 3.0).abs() < 1e-6),
        other => panic!("expected Constant, got {other:?}"),
    }
}

#[test]
fn float_provider_uniform_from_carver_json() {
    let fp: FloatProvider = serde_json::from_str(
        r#"{
                "type": "minecraft:uniform",
                "max_exclusive": 1.4,
                "min_inclusive": 0.7
            }"#,
    )
    .unwrap();
    match fp {
        FloatProvider::Uniform {
            min_inclusive,
            max_exclusive,
        } => {
            assert!((min_inclusive - 0.7).abs() < 1e-6);
            assert!((max_exclusive - 1.4).abs() < 1e-6);
        }
        other => panic!("expected Uniform, got {other:?}"),
    }
}

#[test]
fn float_provider_trapezoid_from_carver_json() {
    let fp: FloatProvider = serde_json::from_str(
        r#"{
                "type": "minecraft:trapezoid",
                "max": 6.0,
                "min": 0.0,
                "plateau": 2.0
            }"#,
    )
    .unwrap();
    match fp {
        FloatProvider::Trapezoid { min, max, plateau } => {
            assert_eq!(min, 0.0);
            assert_eq!(max, 6.0);
            assert_eq!(plateau, 2.0);
        }
        other => panic!("expected Trapezoid, got {other:?}"),
    }
}

#[test]
fn int_provider_clamped_normal_prefers_tagged_shape() {
    let provider: IntProvider = serde_json::from_str(
        r#"{
                "type": "minecraft:clamped_normal",
                "mean": 0.0,
                "deviation": 3.0,
                "min_inclusive": -10,
                "max_inclusive": 10
            }"#,
    )
    .unwrap();

    match provider {
        IntProvider::ClampedNormal {
            mean,
            deviation,
            min_inclusive,
            max_inclusive,
        } => {
            assert_eq!(mean, 0.0);
            assert_eq!(deviation, 3.0);
            assert_eq!(min_inclusive, -10);
            assert_eq!(max_inclusive, 10);
        }
        other => panic!("expected ClampedNormal, got {other:?}"),
    }
}

#[test]
fn provider_type_tags_require_extracted_registry_ids() {
    assert!(
        serde_json::from_str::<HeightProvider>(
            r#"{
                    "type": "uniform",
                    "max_inclusive": {"absolute": 180},
                    "min_inclusive": {"above_bottom": 8}
                }"#,
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<UniformIntProvider>(
            r#"{
                    "type": "uniform",
                    "min_inclusive": 0,
                    "max_inclusive": 10
                }"#,
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<IntProvider>(
            r#"{
                    "type": "uniform",
                    "min_inclusive": 0,
                    "max_inclusive": 10
                }"#,
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<FloatProvider>(
            r#"{
                    "type": "uniform",
                    "min_inclusive": 0.0,
                    "max_exclusive": 1.0
                }"#,
        )
        .is_err()
    );
}

#[test]
fn provider_typed_payloads_deny_unknown_fields() {
    assert!(
        serde_json::from_str::<HeightProvider>(
            r#"{
                    "type": "minecraft:uniform",
                    "max_inclusive": {"absolute": 180},
                    "min_inclusive": {"above_bottom": 8},
                    "extra": 0
                }"#,
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<UniformIntProvider>(
            r#"{
                    "type": "minecraft:uniform",
                    "min_inclusive": 0,
                    "max_inclusive": 10,
                    "extra": 0
                }"#,
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<IntProvider>(
            r#"{
                    "type": "minecraft:clamped",
                    "source": 4,
                    "min_inclusive": 0,
                    "max_inclusive": 10,
                    "extra": 0
                }"#,
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<FloatProvider>(
            r#"{
                    "type": "minecraft:uniform",
                    "min_inclusive": 0.0,
                    "max_exclusive": 1.0,
                    "extra": 0.0
                }"#,
        )
        .is_err()
    );
}

#[test]
fn int_provider_requires_typed_object_or_bare_constant() {
    assert!(
        serde_json::from_str::<IntProvider>(
            r#"{
                    "min_inclusive": 0,
                    "max_inclusive": 10
                }"#,
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<IntProvider>(
            r#"{
                    "type": "minecraft:weighted_list",
                    "distribution": [
                        {
                            "data": 1,
                            "weight": 2,
                            "extra": 3
                        }
                    ]
                }"#,
        )
        .is_err()
    );
}

#[test]
fn int_provider_symmetric_trapezoid_sample_matches_vanilla_shortcut() {
    let provider = IntProvider::Trapezoid {
        min: -7,
        max: 7,
        plateau: 0,
    };
    let mut rng = LegacyRandom::from_seed(123);
    let mut rng_ref = LegacyRandom::from_seed(123);
    let sample = provider.sample(&mut rng);
    let expected = rng_ref.next_i32_bounded(8) - rng_ref.next_i32_bounded(8);
    assert_eq!(sample, expected);
}

/// Matches vanilla's `Mth.randomBetween`: `min + nextFloat()*(max-min)`.
#[test]
fn float_provider_uniform_sample_matches_vanilla() {
    let fp = FloatProvider::Uniform {
        min_inclusive: 0.7,
        max_exclusive: 1.4,
    };
    let mut rng = LegacyRandom::from_seed(0);
    let mut rng_ref = LegacyRandom::from_seed(0);
    let sample = fp.sample(&mut rng);
    let expected = rng_ref.next_f32() * (1.4 - 0.7) + 0.7;
    assert_eq!(sample, expected);
}

/// Height uniform sample: `random.nextInt(max - min + 1) + min`.
#[test]
fn height_provider_uniform_sample_matches_vanilla() {
    let hp = HeightProvider::Uniform {
        min_inclusive: VerticalAnchor::AboveBottom(8),
        max_inclusive: VerticalAnchor::Absolute(180),
    };
    let min_y = -64;
    let height = 384;
    let mut rng = LegacyRandom::from_seed(42);
    let mut rng_ref = LegacyRandom::from_seed(42);
    let sample = hp.sample(&mut rng, min_y, height);
    // min_y + 8 = -56, absolute 180
    let expected = rng_ref.next_i32_between(-56, 180);
    assert_eq!(sample, expected);
}
