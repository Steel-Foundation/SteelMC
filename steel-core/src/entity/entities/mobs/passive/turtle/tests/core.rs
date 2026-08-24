use super::*;

#[test]
fn turtle_registers_vanilla_goal_priorities() {
    let turtle = detached_turtle();

    let selector = turtle.mob_base().goal_selector().lock();
    assert_eq!(selector.available_goal_count(), 9);
    assert_eq!(
        selector.available_goal_priorities(),
        vec![0, 1, 1, 2, 3, 4, 7, 8, 9]
    );
}

#[test]
fn turtle_paths_freely_through_water_and_avoids_doors() {
    let turtle = detached_turtle();

    assert_eq!(turtle.get_pathfinding_malus(PathType::Water), 0.0);
    assert_eq!(turtle.get_pathfinding_malus(PathType::DoorIronClosed), -1.0);
    assert_eq!(turtle.get_pathfinding_malus(PathType::DoorWoodClosed), -1.0);
    assert_eq!(turtle.get_pathfinding_malus(PathType::DoorOpen), -1.0);
}

#[test]
fn turtle_eats_seagrass() {
    let turtle = detached_turtle();

    assert!(turtle.is_food(&ItemStack::new(&vanilla_items::SEAGRASS)));
    assert!(!turtle.is_food(&ItemStack::new(&vanilla_items::WHEAT)));
}

#[test]
fn turtle_carrying_an_egg_cannot_fall_in_love() {
    let turtle = detached_turtle();

    assert!(turtle.can_fall_in_love());

    turtle.set_has_egg(true);
    assert!(!turtle.can_fall_in_love());
}

#[test]
fn set_laying_egg_resets_the_lay_counter() {
    let turtle = detached_turtle();

    turtle.set_laying_egg(true);
    assert!(turtle.is_laying_egg());
    assert_eq!(turtle.lay_egg_counter(), 1);

    turtle.increment_lay_egg_counter();
    assert_eq!(turtle.lay_egg_counter(), 2);

    turtle.set_laying_egg(false);
    assert!(!turtle.is_laying_egg());
    assert_eq!(turtle.lay_egg_counter(), 0);
}
