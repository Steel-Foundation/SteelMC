use super::*;
use crate::{vanilla_blocks, vanilla_items};
use text_components::TextComponent;

fn definition(models: Vec<BedModel>) -> BedRegistration {
    BedRegistration {
        key: Identifier::new_static("bed_test", "bed"),
        template: &vanilla_blocks::BLUE_BED,
        models,
        item: Item::new(
            Identifier::new_static("bed_test", "item"),
            TextComponent::from("Bed".to_owned()),
            None,
        ),
        item_fallback: &vanilla_items::BLUE_BED,
    }
}

#[test]
fn bad_models_and_templates_do_not_leave_partial_beds() {
    let invalid = vec![BedModel {
        properties: vec![("part".into(), "sideways".into())],
        displays: Vec::new(),
    }];
    let overlapping = vec![
        BedModel {
            properties: vec![("part".into(), "head".into())],
            displays: Vec::new(),
        },
        BedModel {
            properties: vec![
                ("part".into(), "head".into()),
                ("facing".into(), "north".into()),
            ],
            displays: Vec::new(),
        },
    ];
    let mut wrong_template = definition(Vec::new());
    wrong_template.template = &vanilla_blocks::OAK_PLANKS;
    let mut straw = definition(Vec::new());
    straw.template = &vanilla_blocks::STRAW_BED;
    for definition in [
        definition(invalid),
        definition(overlapping),
        wrong_template,
        straw,
    ] {
        let mut registry = Registry::new_vanilla();
        let counts = (registry.blocks.next_state_id, registry.items.len());
        assert!(registry.register_client_bed(definition).is_err());
        assert_eq!(
            (registry.blocks.next_state_id, registry.items.len()),
            counts
        );
        assert!(
            registry
                .blocks
                .by_key(&Identifier::new_static("bed_test", "bed"))
                .is_none()
        );
        assert!(
            registry
                .items
                .by_key(&Identifier::new_static("bed_test", "item"))
                .is_none()
        );
    }
}
