use std::{io::Cursor, sync::Arc};

use steel_protocol::packet_traits::EncodedPacket;
use steel_registry::packets::play::C_SET_PLAYER_TEAM;
use steel_utils::{
    codec::VarInt,
    locks::SyncMutex,
    serial::{PrefixedRead as _, ReadFrom as _},
};
use uuid::Uuid;

use crate::entity::next_entity_id;
use crate::player::ResetReason;

use super::{TwoDomains, packet_payloads, test_player_with_uuid_and_packets};

/// Vanilla `ClientboundSetPlayerTeamPacket` method ids.
const METHOD_ADD: i8 = 0;
const METHOD_REMOVE: i8 = 1;

/// Decodes the team name and method of every team packet sent to a player.
fn team_actions(packets: &SyncMutex<Vec<EncodedPacket>>) -> Vec<(String, i8)> {
    packet_payloads(packets, C_SET_PLAYER_TEAM)
        .iter()
        .map(|payload| {
            let mut cursor = Cursor::new(payload.as_slice());
            let Ok(name) = String::read_prefixed::<VarInt>(&mut cursor) else {
                panic!("team name should decode");
            };
            let Ok(method) = i8::read(&mut cursor) else {
                panic!("team method should decode");
            };
            (name, method)
        })
        .collect()
}

#[test]
fn team_packets_follow_the_players_domain() {
    TwoDomains::run("team_sync", async |domains| {
        let (player, packets) = test_player_with_uuid_and_packets(
            &domains.server,
            Arc::clone(domains.source),
            Uuid::from_u128(1),
            "TestPlayer",
            next_entity_id(),
        );
        assert!(domains.server.online_players.insert(Arc::clone(&player)));
        assert!(
            domains
                .source
                .add_player(Arc::clone(&player), ResetReason::InitialJoin)
        );
        let _ = player.mark_joined_world();

        let Some(source) = domains.server.scoreboard("source") else {
            panic!("source domain should have a scoreboard");
        };
        let Some(target) = domains.server.scoreboard("target") else {
            panic!("target domain should have a scoreboard");
        };
        assert!(source.add_team("red").is_ok());
        assert!(target.add_team("blue").is_ok());
        assert_eq!(
            team_actions(&packets),
            [("red".to_owned(), METHOD_ADD)],
            "only the player's own domain reaches it"
        );

        packets.lock().clear();
        let queued = domains
            .server
            .queue_domain_switch(Arc::clone(&player), "target".to_owned());
        assert!(queued.is_ok());
        domains.run_domain_switches().await;

        assert!(domains.target.contains_player(&player));
        assert_eq!(
            team_actions(&packets),
            [
                ("red".to_owned(), METHOD_REMOVE),
                ("blue".to_owned(), METHOD_ADD)
            ]
        );
    });
}
