//! entity query filters, same deal everywhere

use super::Entity;

pub(crate) struct EntitySelector;

impl EntitySelector {
    // TODO: apply vanilla team collision rules once teams are implemented
    pub(crate) fn pushable_by(pusher: &dyn Entity) -> impl Fn(&dyn Entity) -> bool {
        let pusher_id = pusher.id();

        move |entity| entity.id() != pusher_id && !entity.is_spectator() && entity.is_pushable()
    }
}
