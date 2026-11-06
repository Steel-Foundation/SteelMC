// Moved from src/entity/pushing/tests.rs (needs BatEntity at fixture index 6).
#[test]
fn bat_keeps_its_empty_push_entities_override() {
    let reference = Fixture::new(DEFAULT_ENTITIES[6].1, false);
    let actual = Fixture::new(DEFAULT_ENTITIES[6].1, false);
    reference.reset(0);
    actual.reset(0);

    actual.entities[0]
        .as_living_entity()
        .expect("bat")
        .push_entities();

    actual.assert_same_state(&reference, "bat", 0);
}
