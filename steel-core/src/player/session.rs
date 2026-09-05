use std::sync::{
    Arc, Weak,
    atomic::{AtomicU64, Ordering},
};

use steel_utils::locks::{SyncMutex, SyncRwLock};

use super::{
    DROP_SPAM_THROTTLER_INCREMENT_STEP, DROP_SPAM_THROTTLER_THRESHOLD, Player, chat::ChatState,
    chunk_sender::ChunkSender, spam_throttler::TickThrottler,
};

static LAST_PLAYER_SESSION_ID: AtomicU64 = AtomicU64::new(0);

/// Runtime identity for one connected client session.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct PlayerSessionId(u64);

/// Connection-owned state that survives replacement of the active player entity.
pub struct PlayerSession {
    id: PlayerSessionId,
    current_player: SyncRwLock<CurrentPlayerSlot>,
    pub(crate) chunk_sender: SyncMutex<ChunkSender>,
    pub(crate) chat: SyncMutex<ChatState>,
    /// Vanilla keeps creative drop throttling on the connection across respawns.
    pub(super) drop_spam_throttler: SyncMutex<TickThrottler>,
}

enum CurrentPlayerSlot {
    Unbound,
    Bound(Weak<Player>),
    Closed,
}

impl PlayerSession {
    /// Creates an unbound session for a newly accepted client connection.
    ///
    /// # Panics
    ///
    /// Panics if the process exhausts the player-session ID space.
    #[must_use]
    pub fn new(chat_spam_threshold_seconds: i32, command_spam_threshold_seconds: i32) -> Self {
        let Ok(previous) =
            LAST_PLAYER_SESSION_ID.try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
        else {
            panic!("player session ID space exhausted");
        };

        Self {
            id: PlayerSessionId(previous + 1),
            current_player: SyncRwLock::new(CurrentPlayerSlot::Unbound),
            chunk_sender: SyncMutex::new(ChunkSender::default()),
            chat: SyncMutex::new(ChatState::new(
                chat_spam_threshold_seconds,
                command_spam_threshold_seconds,
            )),
            drop_spam_throttler: SyncMutex::new(TickThrottler::new(
                DROP_SPAM_THROTTLER_INCREMENT_STEP,
                DROP_SPAM_THROTTLER_THRESHOLD,
            )),
        }
    }

    #[must_use]
    pub(crate) const fn id(&self) -> PlayerSessionId {
        self.id
    }

    pub(crate) const fn chunk_sender(&self) -> &SyncMutex<ChunkSender> {
        &self.chunk_sender
    }

    /// Returns the player entity currently controlled by this connection.
    #[must_use]
    pub(crate) fn current_player(&self) -> Option<Arc<Player>> {
        match &*self.current_player.read() {
            CurrentPlayerSlot::Bound(player) => player.upgrade(),
            CurrentPlayerSlot::Unbound | CurrentPlayerSlot::Closed => None,
        }
    }

    /// Returns whether `player` is the entity currently controlled by this session.
    #[must_use]
    pub(crate) fn is_current_player(&self, player: &Arc<Player>) -> bool {
        self.owns(player)
            && matches!(
                &*self.current_player.read(),
                CurrentPlayerSlot::Bound(current) if current.ptr_eq(&Arc::downgrade(player))
            )
    }

    /// Binds the first player entity created for this connection.
    pub fn bind_initial_player(&self, player: &Arc<Player>) -> bool {
        if !self.owns(player) {
            return false;
        }

        let mut current = self.current_player.write();
        if !matches!(*current, CurrentPlayerSlot::Unbound) {
            return false;
        }
        *current = CurrentPlayerSlot::Bound(Arc::downgrade(player));
        true
    }

    /// Rebinds the connection only if `expected` is still its active player entity.
    pub(crate) fn replace_player(&self, expected: &Arc<Player>, replacement: &Arc<Player>) -> bool {
        if !self.owns(expected) || !self.owns(replacement) {
            return false;
        }

        let mut current = self.current_player.write();
        if !matches!(
            &*current,
            CurrentPlayerSlot::Bound(player) if player.ptr_eq(&Arc::downgrade(expected))
        ) {
            return false;
        }
        *current = CurrentPlayerSlot::Bound(Arc::downgrade(replacement));
        true
    }

    /// Clears the active player only if `expected` still owns this connection.
    pub(crate) fn clear_player(&self, expected: &Arc<Player>) -> bool {
        if !self.owns(expected) {
            return false;
        }

        let mut current = self.current_player.write();
        if !matches!(
            &*current,
            CurrentPlayerSlot::Bound(player) if player.ptr_eq(&Arc::downgrade(expected))
        ) {
            return false;
        }
        *current = CurrentPlayerSlot::Closed;
        true
    }

    fn owns(&self, player: &Player) -> bool {
        self.id == player.session.id
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        entity::Entity as _,
        player::{ClientInformation, Player},
        test_support::{TestPlayerBuilder, fresh_test_world},
    };

    use super::PlayerSession;

    fn replacement_for(player: &Arc<Player>, session: Arc<PlayerSession>) -> Arc<Player> {
        Arc::new(Player::new(
            player.gameprofile.clone(),
            Arc::clone(&player.connection),
            session,
            player.get_world(),
            player.server.clone(),
            Arc::clone(&player.config),
            player.id(),
            ClientInformation::default(),
        ))
    }

    #[test]
    fn replacement_requires_session_ownership_and_exact_current_player() {
        let world = fresh_test_world("player_session_exact_replacement");
        let original = TestPlayerBuilder::new(Arc::clone(&world), "Original", 1).build();
        let session = Arc::clone(&original.session);
        let replacement = replacement_for(&original, Arc::clone(&session));
        let stale_replacement = replacement_for(&original, Arc::clone(&session));
        let foreign = TestPlayerBuilder::new(world, "Foreign", 2).build();
        original.chat().lock().messages_sent = 7;

        assert!(!session.replace_player(&original, &foreign));
        assert!(session.replace_player(&original, &replacement));
        assert!(session.is_current_player(&replacement));
        assert!(!session.is_current_player(&original));
        assert!(!session.replace_player(&original, &stale_replacement));
        assert_eq!(replacement.chat().lock().messages_sent, 7);

        let Some(current) = session.current_player() else {
            panic!("replacement should remain bound to the session");
        };
        assert!(Arc::ptr_eq(&current, &replacement));
    }

    #[test]
    fn closed_session_cannot_be_bound_again() {
        let session = Arc::new(PlayerSession::new(10, 10));
        let foreign = TestPlayerBuilder::new(
            fresh_test_world("player_session_foreign_initial_bind"),
            "Foreign",
            3,
        )
        .build();
        assert!(!session.bind_initial_player(&foreign));

        let player = replacement_for(&foreign, Arc::clone(&session));
        assert!(session.bind_initial_player(&player));
        assert!(session.clear_player(&player));
        assert!(session.current_player().is_none());

        let replacement = replacement_for(&player, Arc::clone(&session));
        assert!(!session.bind_initial_player(&replacement));
        assert!(!session.replace_player(&player, &replacement));
        assert!(!session.clear_player(&player));
    }
}
