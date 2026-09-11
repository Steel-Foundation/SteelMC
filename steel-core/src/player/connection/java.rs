//! This module contains the `JavaConnection` struct, which is used to represent a connection to a Java client.
use std::io::Cursor;
use std::sync::{Arc, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use steel_protocol::packet_reader::TCPNetworkDecoder;
use steel_protocol::packet_traits::{ClientPacket, CompressionInfo, EncodedPacket, ServerPacket};
use steel_protocol::packet_writer::TCPNetworkEncoder;
use steel_protocol::packets::common::{
    CDisconnect, CKeepAlive, CPongResponse, SClientInformation, SCustomPayload, SKeepAlive,
    SPingRequest,
};
use steel_protocol::packets::game::{
    CBundleDelimiter, CCommandSuggestions, ClientCommandAction, PlayerAction, PlayerCommandAction,
    SAcceptTeleportation, SAttack, SChangeDifficulty, SChangeGameMode, SChat, SChatAck,
    SChatCommand, SChatSessionUpdate, SChunkBatchReceived, SClientCommand, SClientTickEnd,
    SCommandSuggestion, SContainerButtonClick, SContainerClick, SContainerClose,
    SContainerSlotStateChanged, SInteract, SMovePlayer, SMovePlayerPos, SMovePlayerPosRot,
    SMovePlayerRot, SMovePlayerStatusOnly, SMoveVehicle, SPickItemFromBlock, SPlayerAbilities,
    SPlayerAction, SPlayerCommand, SPlayerInput, SPlayerLoad, SRenameItem, SSetCarriedItem,
    SSetCreativeModeSlot, SSignUpdate, SSpectatorAction, SSwing, SUseItem, SUseItemOn,
};

use steel_protocol::utils::{ConnectionProtocol, PacketError, RawPacket};
use steel_registry::packets::play;
use steel_utils::locks::{AsyncMutex, SyncMutex};
use steel_utils::translations;
use text_components::content::Resolvable;
use text_components::custom::CustomData;
use text_components::resolving::TextResolutor;
use text_components::{Modifier, TextComponent, format::Color};
use tokio::io::{BufReader, BufWriter};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::select;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, error::TryRecvError};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::command::{handle_client_request, sender::CommandSender};
use crate::player::Player;
use crate::player::connection::NetworkConnection;
use crate::server::Server;

/// Shared Java socket writer.
pub type JavaNetworkWriter = Arc<AsyncMutex<Option<TCPNetworkEncoder<BufWriter<OwnedWriteHalf>>>>>;

const DISCONNECT_FLUSH_TIMEOUT: Duration = Duration::from_secs(1);

/// Outbound packet queue message for Java connections.
pub enum OutboundPacket {
    /// Normal packet write that may be interrupted by connection shutdown.
    Packet(EncodedPacket),
    /// Final disconnect packet that is flushed on a bounded best-effort basis.
    Disconnect(EncodedPacket),
}

/// A decoded play packet whose handler runs in the server's inter-tick packet phase.
///
/// Carries its [`PlayPacketDescriptor`] so routing metadata (execution class, join gates,
/// handler) is resolved once at decode time.
pub(crate) struct ScheduledPlayPacket {
    descriptor: &'static PlayPacketDescriptor,
    kind: ScheduledPlayPacketKind,
}

/// Cross-player concurrency permitted for a scheduled packet handler.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScheduledPacketExecution {
    /// The handler may overlap handlers for other players, but never its own player lane.
    ///
    /// Shared mutations must be fully linearized by their resource locks, and the handler must
    /// tolerate cross-player execution order differing from packet submission order.
    PlayerLocal,
    /// The handler may overlap player-local work, but not another serialized handler. Serialized
    /// handlers start in global packet submission order.
    Serialized,
    /// The handler is a global submission-order barrier and must not overlap scheduled work.
    Exclusive,
}

pub(crate) enum ScheduledPlayPacketKind {
    AcceptTeleportation(SAcceptTeleportation),
    Attack(SAttack),
    Interact(SInteract),
    CustomPayload(SCustomPayload),
    Chat(Box<SChat>),
    ChatAck(SChatAck),
    ChatSessionUpdate(SChatSessionUpdate),
    ClientInformation(SClientInformation),
    ClientTickEnd,
    MovePlayer(SMovePlayer),
    MoveVehicle(SMoveVehicle),
    PlayerLoaded,
    ChatCommand(SChatCommand),
    CommandSuggestion(SCommandSuggestion),
    ContainerButtonClick(SContainerButtonClick),
    ContainerClick(SContainerClick),
    ContainerClose(SContainerClose),
    ContainerSlotStateChanged(SContainerSlotStateChanged),
    SetCreativeModeSlot(SSetCreativeModeSlot),
    PlayerInput(SPlayerInput),
    PlayerCommand(SPlayerCommand),
    PlayerAbilities(SPlayerAbilities),
    RenameItem(SRenameItem),
    UseItemOn(SUseItemOn),
    UseItem(SUseItem),
    SetCarriedItem(SSetCarriedItem),
    Swing(SSwing),
    PlayerAction(SPlayerAction),
    PickItemFromBlock(SPickItemFromBlock),
    SignUpdate(SSignUpdate),
    SpectatorAction(SSpectatorAction),
    ClientCommand(SClientCommand),
    ChangeGameMode(SChangeGameMode),
    ChangeDifficulty(SChangeDifficulty),
}

enum ImmediatePlayPacket {
    KeepAlive(SKeepAlive),
    PingRequest(SPingRequest),
    ChunkBatchReceived(SChunkBatchReceived),
    Unknown(i32),
}

enum DecodedPlayPacket {
    Scheduled(ScheduledPlayPacket),
    Immediate(ImmediatePlayPacket),
}

/// Declarative routing descriptor for one scheduled serverbound play packet.
///
/// The table is the single source of truth for how a packet is decoded, which
/// cross-player concurrency class its handler runs in, whether it may run before the
/// player finished joining or during a domain-switch handshake, and how its handler is
/// invoked. Adding a packet means adding one entry; routing and concurrency cannot
/// drift apart.
pub(crate) struct PlayPacketDescriptor {
    /// Serverbound play packet ID.
    pub(crate) id: i32,
    /// Decodes the packet payload into the scheduled kind.
    pub(crate) decode: fn(&mut Cursor<&[u8]>) -> Result<ScheduledPlayPacketKind, PacketError>,
    /// Cross-player concurrency class for the inter-tick phase.
    pub(crate) execution: fn(&ScheduledPlayPacketKind) -> ScheduledPacketExecution,
    /// Whether the handler may run before the player finished joining the world.
    pub(crate) can_process_before_join: bool,
    /// Whether the handler may run during a domain-switch handshake.
    pub(crate) can_process_during_domain_handshake: bool,
    /// Runs the handler.
    pub(crate) handle: fn(ScheduledPlayPacketKind, Arc<Player>, &Arc<Server>),
}

fn player_command_execution(kind: &ScheduledPlayPacketKind) -> ScheduledPacketExecution {
    let ScheduledPlayPacketKind::PlayerCommand(packet) = kind else {
        unreachable!("player command descriptor only evaluates its own kind");
    };
    match packet.action {
        PlayerCommandAction::StartSprinting
        | PlayerCommandAction::StopSprinting
        | PlayerCommandAction::StartFallFlying => ScheduledPacketExecution::PlayerLocal,
        PlayerCommandAction::LeaveBed => ScheduledPacketExecution::Serialized,
        // These handlers are not implemented, so their eventual vehicle transaction
        // cannot yet be audited against concurrently player-local work.
        PlayerCommandAction::StartRidingJump
        | PlayerCommandAction::StopRidingJump
        | PlayerCommandAction::OpenVehicleInventory => ScheduledPacketExecution::Exclusive,
    }
}

fn player_action_execution(kind: &ScheduledPlayPacketKind) -> ScheduledPacketExecution {
    let ScheduledPlayPacketKind::PlayerAction(packet) = kind else {
        unreachable!("player action descriptor only evaluates its own kind");
    };
    match packet.action {
        PlayerAction::AbortDestroyBlock | PlayerAction::SwapItemWithOffhand => {
            ScheduledPacketExecution::PlayerLocal
        }
        PlayerAction::StartDestroyBlock
        | PlayerAction::StopDestroyBlock
        | PlayerAction::DropAllItems
        | PlayerAction::DropItem => ScheduledPacketExecution::Serialized,
        // Active-use release may invoke item behavior and mutate inventory, while stab
        // spans independently locked targets; neither can overlap player-local work.
        PlayerAction::ReleaseUseItem | PlayerAction::Stab => ScheduledPacketExecution::Exclusive,
    }
}

macro_rules! simple_execution {
    ($class:expr) => {
        |_: &ScheduledPlayPacketKind| $class
    };
}

const ACCEPT_TELEPORTATION_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_ACCEPT_TELEPORTATION,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::AcceptTeleportation(
            SAcceptTeleportation::read_packet(data)?,
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::Serialized),
    can_process_before_join: true,
    can_process_during_domain_handshake: true,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::AcceptTeleportation(packet) => {
            player.handle_accept_teleportation(packet);
        }
        _ => unreachable!("accept teleportation descriptor routed a different kind"),
    },
};

const ATTACK_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_ATTACK,
    decode: |data| Ok(ScheduledPlayPacketKind::Attack(SAttack::read_packet(data)?)),
    execution: simple_execution!(ScheduledPacketExecution::Exclusive),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::Attack(packet) => player.handle_attack(packet),
        _ => unreachable!("attack descriptor routed a different kind"),
    },
};

const INTERACT_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_INTERACT,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::Interact(SInteract::read_packet(
            data,
        )?))
    },
    execution: simple_execution!(ScheduledPacketExecution::Exclusive),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::Interact(packet) => player.handle_interact(packet),
        _ => unreachable!("interact descriptor routed a different kind"),
    },
};

const CUSTOM_PAYLOAD_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_CUSTOM_PAYLOAD,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::CustomPayload(
            SCustomPayload::read_packet(data)?,
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::Exclusive),
    can_process_before_join: true,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::CustomPayload(packet) => player.handle_custom_payload(packet),
        _ => unreachable!("custom payload descriptor routed a different kind"),
    },
};

const CHAT_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_CHAT,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::Chat(Box::new(SChat::read_packet(
            data,
        )?)))
    },
    execution: simple_execution!(ScheduledPacketExecution::Serialized),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::Chat(packet) => player.handle_chat(*packet, Arc::clone(&player)),
        _ => unreachable!("chat descriptor routed a different kind"),
    },
};

const CHAT_SESSION_UPDATE_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_CHAT_SESSION_UPDATE,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::ChatSessionUpdate(
            SChatSessionUpdate::read_packet(data)?,
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::PlayerLocal),
    can_process_before_join: true,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::ChatSessionUpdate(packet) => {
            player.handle_chat_session_update(packet);
        }
        _ => unreachable!("chat session update descriptor routed a different kind"),
    },
};

const CHAT_ACK_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_CHAT_ACK,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::ChatAck(SChatAck::read_packet(
            data,
        )?))
    },
    execution: simple_execution!(ScheduledPacketExecution::Serialized),
    can_process_before_join: true,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::ChatAck(packet) => player.handle_chat_ack(packet),
        _ => unreachable!("chat ack descriptor routed a different kind"),
    },
};

const CLIENT_INFORMATION_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_CLIENT_INFORMATION,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::ClientInformation(
            SClientInformation::read_packet(data)?,
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::PlayerLocal),
    can_process_before_join: true,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::ClientInformation(packet) => {
            player.handle_client_information(packet);
        }
        _ => unreachable!("client information descriptor routed a different kind"),
    },
};

const CLIENT_TICK_END_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_CLIENT_TICK_END,
    decode: |data| {
        let _ = SClientTickEnd::read_packet(data)?;
        Ok(ScheduledPlayPacketKind::ClientTickEnd)
    },
    execution: simple_execution!(ScheduledPacketExecution::PlayerLocal),
    can_process_before_join: true,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::ClientTickEnd => player.handle_client_tick_end(),
        _ => unreachable!("client tick end descriptor routed a different kind"),
    },
};

const MOVE_PLAYER_POS_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_MOVE_PLAYER_POS,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::MovePlayer(
            SMovePlayerPos::read_packet(data)?.into(),
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::Serialized),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::MovePlayer(packet) => player.handle_move_player(packet),
        _ => unreachable!("move player descriptor routed a different kind"),
    },
};

const MOVE_PLAYER_POS_ROT_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_MOVE_PLAYER_POS_ROT,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::MovePlayer(
            SMovePlayerPosRot::read_packet(data)?.into(),
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::Serialized),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: MOVE_PLAYER_POS_PACKET.handle,
};

const MOVE_PLAYER_ROT_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_MOVE_PLAYER_ROT,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::MovePlayer(
            SMovePlayerRot::read_packet(data)?.into(),
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::Serialized),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: MOVE_PLAYER_POS_PACKET.handle,
};

const MOVE_PLAYER_STATUS_ONLY_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_MOVE_PLAYER_STATUS_ONLY,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::MovePlayer(
            SMovePlayerStatusOnly::read_packet(data)?.into(),
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::Serialized),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: MOVE_PLAYER_POS_PACKET.handle,
};

const MOVE_VEHICLE_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_MOVE_VEHICLE,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::MoveVehicle(
            SMoveVehicle::read_packet(data)?,
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::Serialized),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::MoveVehicle(packet) => player.handle_move_vehicle(packet),
        _ => unreachable!("move vehicle descriptor routed a different kind"),
    },
};

const PLAYER_LOADED_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_PLAYER_LOADED,
    decode: |data| {
        let _ = SPlayerLoad::read_packet(data)?;
        Ok(ScheduledPlayPacketKind::PlayerLoaded)
    },
    execution: simple_execution!(ScheduledPacketExecution::PlayerLocal),
    can_process_before_join: true,
    can_process_during_domain_handshake: true,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::PlayerLoaded => {
            if player.mark_client_loaded_from_network() {
                player.send_inventory_to_remote();
            }
        }
        _ => unreachable!("player loaded descriptor routed a different kind"),
    },
};

const CHAT_COMMAND_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_CHAT_COMMAND,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::ChatCommand(
            SChatCommand::read_packet(data)?,
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::PlayerLocal),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, server| match packet {
        ScheduledPlayPacketKind::ChatCommand(packet) => {
            player.reset_last_action_time();
            if server
                .submit_command(CommandSender::Player(Arc::clone(&player)), packet.command)
                .is_err()
            {
                player.send_message(
                    &TextComponent::const_plain("Command queue is full").color(Color::Red),
                );
            }
            player.detect_command_rate_spam();
        }
        _ => unreachable!("chat command descriptor routed a different kind"),
    },
};

const COMMAND_SUGGESTION_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_COMMAND_SUGGESTION,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::CommandSuggestion(
            SCommandSuggestion::read_packet(data)?,
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::PlayerLocal),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, server| match packet {
        ScheduledPlayPacketKind::CommandSuggestion(packet) => {
            if server
                .submit_command_suggestions(Arc::clone(&player), packet.id, packet.command)
                .is_err()
            {
                player.send_packet(CCommandSuggestions::new(packet.id, 0, 0, Vec::new()));
            }
        }
        _ => unreachable!("command suggestion descriptor routed a different kind"),
    },
};

const CONTAINER_BUTTON_CLICK_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_CONTAINER_BUTTON_CLICK,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::ContainerButtonClick(
            SContainerButtonClick::read_packet(data)?,
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::Exclusive),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::ContainerButtonClick(packet) => {
            player.handle_container_button_click(packet);
        }
        _ => unreachable!("container button click descriptor routed a different kind"),
    },
};

const CONTAINER_CLICK_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_CONTAINER_CLICK,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::ContainerClick(
            SContainerClick::read_packet(data)?,
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::Serialized),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::ContainerClick(packet) => player.handle_container_click(packet),
        _ => unreachable!("container click descriptor routed a different kind"),
    },
};

const CONTAINER_CLOSE_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_CONTAINER_CLOSE,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::ContainerClose(
            SContainerClose::read_packet(data)?,
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::PlayerLocal),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::ContainerClose(packet) => player.handle_container_close(packet),
        _ => unreachable!("container close descriptor routed a different kind"),
    },
};

const CONTAINER_SLOT_STATE_CHANGED_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_CONTAINER_SLOT_STATE_CHANGED,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::ContainerSlotStateChanged(
            SContainerSlotStateChanged::read_packet(data)?,
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::Exclusive),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::ContainerSlotStateChanged(packet) => {
            player.handle_container_slot_state_changed(packet);
        }
        _ => unreachable!("container slot state changed descriptor routed a different kind"),
    },
};

const SET_CREATIVE_MODE_SLOT_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_SET_CREATIVE_MODE_SLOT,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::SetCreativeModeSlot(
            SSetCreativeModeSlot::read_packet(data)?,
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::PlayerLocal),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::SetCreativeModeSlot(packet) => {
            player.handle_set_creative_mode_slot(packet);
        }
        _ => unreachable!("set creative mode slot descriptor routed a different kind"),
    },
};

const PLAYER_INPUT_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_PLAYER_INPUT,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::PlayerInput(
            SPlayerInput::read_packet(data)?,
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::PlayerLocal),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::PlayerInput(packet) => player.handle_player_input(packet),
        _ => unreachable!("player input descriptor routed a different kind"),
    },
};

const PLAYER_COMMAND_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_PLAYER_COMMAND,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::PlayerCommand(
            SPlayerCommand::read_packet(data)?,
        ))
    },
    execution: player_command_execution,
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::PlayerCommand(packet) => player.handle_player_command(packet),
        _ => unreachable!("player command descriptor routed a different kind"),
    },
};

const PLAYER_ABILITIES_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_PLAYER_ABILITIES,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::PlayerAbilities(
            SPlayerAbilities::read_packet(data)?,
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::PlayerLocal),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::PlayerAbilities(packet) => player.handle_player_abilities(packet),
        _ => unreachable!("player abilities descriptor routed a different kind"),
    },
};

const RENAME_ITEM_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_RENAME_ITEM,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::RenameItem(
            SRenameItem::read_packet(data)?,
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::Serialized),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::RenameItem(packet) => player.handle_rename_item(packet),
        _ => unreachable!("rename item descriptor routed a different kind"),
    },
};

const USE_ITEM_ON_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_USE_ITEM_ON,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::UseItemOn(SUseItemOn::read_packet(
            data,
        )?))
    },
    execution: simple_execution!(ScheduledPacketExecution::Serialized),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::UseItemOn(packet) => player.handle_use_item_on(packet),
        _ => unreachable!("use item on descriptor routed a different kind"),
    },
};

const USE_ITEM_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_USE_ITEM,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::UseItem(SUseItem::read_packet(
            data,
        )?))
    },
    execution: simple_execution!(ScheduledPacketExecution::Serialized),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::UseItem(packet) => player.handle_use_item(packet),
        _ => unreachable!("use item descriptor routed a different kind"),
    },
};

const SET_CARRIED_ITEM_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_SET_CARRIED_ITEM,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::SetCarriedItem(
            SSetCarriedItem::read_packet(data)?,
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::PlayerLocal),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::SetCarriedItem(packet) => player.handle_set_carried_item(packet),
        _ => unreachable!("set carried item descriptor routed a different kind"),
    },
};

const SWING_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_SWING,
    decode: |data| Ok(ScheduledPlayPacketKind::Swing(SSwing::read_packet(data)?)),
    execution: simple_execution!(ScheduledPacketExecution::PlayerLocal),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::Swing(packet) => player.handle_animate(packet),
        _ => unreachable!("swing descriptor routed a different kind"),
    },
};

const PLAYER_ACTION_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_PLAYER_ACTION,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::PlayerAction(
            SPlayerAction::read_packet(data)?,
        ))
    },
    execution: player_action_execution,
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::PlayerAction(packet) => player.handle_player_action(packet),
        _ => unreachable!("player action descriptor routed a different kind"),
    },
};

const PICK_ITEM_FROM_BLOCK_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_PICK_ITEM_FROM_BLOCK,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::PickItemFromBlock(
            SPickItemFromBlock::read_packet(data)?,
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::PlayerLocal),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::PickItemFromBlock(packet) => {
            player.handle_pick_item_from_block(packet);
        }
        _ => unreachable!("pick item from block descriptor routed a different kind"),
    },
};

const SIGN_UPDATE_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_SIGN_UPDATE,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::SignUpdate(
            SSignUpdate::read_packet(data)?,
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::Serialized),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::SignUpdate(packet) => player.handle_sign_update(packet),
        _ => unreachable!("sign update descriptor routed a different kind"),
    },
};

const SPECTATOR_ACTION_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_SPECTATOR_ACTION,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::SpectatorAction(
            SSpectatorAction::read_packet(data)?,
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::Serialized),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::SpectatorAction(packet) => player.handle_spectator_action(packet),
        _ => unreachable!("spectator action descriptor routed a different kind"),
    },
};

const CLIENT_COMMAND_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_CLIENT_COMMAND,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::ClientCommand(
            SClientCommand::read_packet(data)?,
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::PlayerLocal),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::ClientCommand(packet) => {
            player.handle_client_command(packet.action);
        }
        _ => unreachable!("client command descriptor routed a different kind"),
    },
};

const CHANGE_GAME_MODE_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_CHANGE_GAME_MODE,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::ChangeGameMode(
            SChangeGameMode::read_packet(data)?,
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::Serialized),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, server| match packet {
        ScheduledPlayPacketKind::ChangeGameMode(packet) => {
            handle_client_request(&player, server, packet.gamemode);
        }
        _ => unreachable!("change game mode descriptor routed a different kind"),
    },
};

const CHANGE_DIFFICULTY_PACKET: PlayPacketDescriptor = PlayPacketDescriptor {
    id: play::S_CHANGE_DIFFICULTY,
    decode: |data| {
        Ok(ScheduledPlayPacketKind::ChangeDifficulty(
            SChangeDifficulty::read_packet(data)?,
        ))
    },
    execution: simple_execution!(ScheduledPacketExecution::Serialized),
    can_process_before_join: false,
    can_process_during_domain_handshake: false,
    handle: |packet, player, _server| match packet {
        ScheduledPlayPacketKind::ChangeDifficulty(packet) => {
            player.handle_change_difficulty(packet.difficulty);
        }
        _ => unreachable!("change difficulty descriptor routed a different kind"),
    },
};

/// Every scheduled serverbound play packet, keyed by [`PlayPacketDescriptor::id`].
const PLAY_PACKET_DESCRIPTORS: &[PlayPacketDescriptor] = &[
    ACCEPT_TELEPORTATION_PACKET,
    ATTACK_PACKET,
    INTERACT_PACKET,
    CUSTOM_PAYLOAD_PACKET,
    CHAT_PACKET,
    CHAT_SESSION_UPDATE_PACKET,
    CHAT_ACK_PACKET,
    CLIENT_INFORMATION_PACKET,
    CLIENT_TICK_END_PACKET,
    MOVE_PLAYER_POS_PACKET,
    MOVE_PLAYER_POS_ROT_PACKET,
    MOVE_PLAYER_ROT_PACKET,
    MOVE_PLAYER_STATUS_ONLY_PACKET,
    MOVE_VEHICLE_PACKET,
    PLAYER_LOADED_PACKET,
    CHAT_COMMAND_PACKET,
    COMMAND_SUGGESTION_PACKET,
    CONTAINER_BUTTON_CLICK_PACKET,
    CONTAINER_CLICK_PACKET,
    CONTAINER_CLOSE_PACKET,
    CONTAINER_SLOT_STATE_CHANGED_PACKET,
    SET_CREATIVE_MODE_SLOT_PACKET,
    PLAYER_INPUT_PACKET,
    PLAYER_COMMAND_PACKET,
    PLAYER_ABILITIES_PACKET,
    RENAME_ITEM_PACKET,
    USE_ITEM_ON_PACKET,
    USE_ITEM_PACKET,
    SET_CARRIED_ITEM_PACKET,
    SWING_PACKET,
    PLAYER_ACTION_PACKET,
    PICK_ITEM_FROM_BLOCK_PACKET,
    SIGN_UPDATE_PACKET,
    SPECTATOR_ACTION_PACKET,
    CLIENT_COMMAND_PACKET,
    CHANGE_GAME_MODE_PACKET,
    CHANGE_DIFFICULTY_PACKET,
];

/// Resolves the routing descriptor for a serverbound play packet ID.
fn play_packet_descriptor(id: i32) -> Option<&'static PlayPacketDescriptor> {
    PLAY_PACKET_DESCRIPTORS.iter().find(|d| d.id == id)
}

impl ScheduledPlayPacket {
    /// Returns whether this packet acknowledges target-world synchronization.
    pub(crate) const fn is_domain_handshake_packet(&self) -> bool {
        matches!(
            self.kind,
            ScheduledPlayPacketKind::AcceptTeleportation(_) | ScheduledPlayPacketKind::PlayerLoaded
        )
    }

    /// Returns whether this is the death screen's one-shot respawn request.
    pub(crate) const fn is_perform_respawn(&self) -> bool {
        matches!(
            self.kind,
            ScheduledPlayPacketKind::ClientCommand(SClientCommand {
                action: ClientCommandAction::PerformRespawn,
            })
        )
    }

    #[cfg(test)]
    pub(crate) const fn perform_respawn_for_test() -> Self {
        Self {
            descriptor: &CLIENT_COMMAND_PACKET,
            kind: ScheduledPlayPacketKind::ClientCommand(SClientCommand {
                action: ClientCommandAction::PerformRespawn,
            }),
        }
    }

    /// Returns the handler's audited cross-player concurrency class, resolved from the
    /// packet's [`PlayPacketDescriptor`].
    pub(crate) fn execution(&self) -> ScheduledPacketExecution {
        (self.descriptor.execution)(&self.kind)
    }

    pub(crate) const fn can_process_before_join(&self) -> bool {
        self.descriptor.can_process_before_join
    }

    pub(crate) fn handle(self, player: Arc<Player>, server: &Arc<Server>) {
        if !player.has_joined_world() && !self.can_process_before_join() {
            return;
        }

        (self.descriptor.handle)(self.kind, player, server);
    }
    pub(crate) const fn new(
        descriptor: &'static PlayPacketDescriptor,
        kind: ScheduledPlayPacketKind,
    ) -> Self {
        Self { descriptor, kind }
    }
}

/// Builder for creating packet bundles.
///
/// Used with [`JavaConnection::send_bundle`] to send multiple packets atomically.
pub struct BundleBuilder {
    packets: Vec<EncodedPacket>,
    compression: Option<CompressionInfo>,
}

impl BundleBuilder {
    /// Creates a new `BundleBuilder` with the given compression settings.
    #[must_use]
    pub const fn new(compression: Option<CompressionInfo>) -> Self {
        Self {
            packets: Vec::new(),
            compression,
        }
    }

    /// Adds a packet to the bundle.
    ///
    /// # Panics
    /// Panics if the packet fails to encode.
    pub fn add<P: ClientPacket>(&mut self, packet: P) {
        let encoded = EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
            .expect("Failed to encode packet");
        self.packets.push(encoded);
    }

    /// Consumes the builder and returns the collected encoded packets.
    #[must_use]
    pub fn into_packets(self) -> Vec<EncodedPacket> {
        self.packets
    }
}

#[expect(
    clippy::struct_field_names,
    reason = "alive_ prefix is intentional to group related keep-alive fields"
)]
struct KeepAliveTracker {
    alive_time: u64,
    alive_pending: bool,
    alive_id: u64,
}

/// A connection to a Java client.
pub struct JavaConnection {
    outgoing_packets: UnboundedSender<OutboundPacket>,
    cancel_token: CancellationToken,
    compression: Option<CompressionInfo>,
    network_writer: JavaNetworkWriter,
    id: u64,

    player: Weak<Player>,
    keep_alive_tracker: SyncMutex<KeepAliveTracker>,
    latency: SyncMutex<u32>,
}

impl JavaConnection {
    /// Creates a new `JavaConnection`.
    pub const fn new(
        outgoing_packets: UnboundedSender<OutboundPacket>,
        cancel_token: CancellationToken,
        compression: Option<CompressionInfo>,
        network_writer: JavaNetworkWriter,
        id: u64,
        player: Weak<Player>,
    ) -> Self {
        Self {
            outgoing_packets,
            cancel_token,
            compression,
            network_writer,
            id,
            player,
            keep_alive_tracker: SyncMutex::new(KeepAliveTracker {
                alive_time: 0,
                alive_pending: false,
                alive_id: 0,
            }),
            latency: SyncMutex::new(0),
        }
    }

    async fn write_packet_now(&self, packet: &EncodedPacket) -> Result<(), PacketError> {
        let mut network_writer = self.network_writer.lock().await;
        let Some(network_writer) = network_writer.as_mut() else {
            return Err(PacketError::ConnectionClosed);
        };
        network_writer.write_packet(packet).await
    }

    async fn finish_disconnect(&self, disconnect_packet: Option<EncodedPacket>) {
        let finish = async {
            let Some(mut network_writer) = self.network_writer.lock().await.take() else {
                return Ok(());
            };
            let Some(packet) = disconnect_packet else {
                return Ok(());
            };
            network_writer.write_packet(&packet).await
        };

        match timeout(DISCONNECT_FLUSH_TIMEOUT, finish).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => log::debug!(
                "Best-effort disconnect write for client {} failed: {error}",
                self.id
            ),
            Err(_) => log::debug!(
                "Best-effort disconnect write for client {} timed out",
                self.id
            ),
        }
    }

    /// Ticks the connection.
    pub fn tick(&self) {
        self.keep_connection_alive();
    }

    fn keep_connection_alive(&self) {
        let mut tracker = self.keep_alive_tracker.lock();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time before UNIX EPOCH")
            .as_millis() as u64;

        if now - tracker.alive_time >= 15000 {
            if tracker.alive_pending {
                self.disconnect(translations::DISCONNECT_TIMEOUT.msg());
            } else {
                tracker.alive_pending = true;
                tracker.alive_id = now;
                tracker.alive_time = now;
                self.send_packet(CKeepAlive::new(tracker.alive_id as i64));
            }
        }
    }

    /// Handles a keep alive packet.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "latency saturates at u32::MAX ms (~49 days), which is unreachable in practice"
    )]
    fn handle_keep_alive(&self, packet: SKeepAlive) {
        let mut tracker = self.keep_alive_tracker.lock();
        if tracker.alive_pending && packet.id as u64 == tracker.alive_id {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("System time before UNIX EPOCH")
                .as_millis() as u64;

            let time = now.saturating_sub(tracker.alive_time) as u32;
            tracker.alive_pending = false;
            drop(tracker);
            let mut latency = self.latency.lock();
            *latency = (*latency * 3 + time) / 4;
        } else {
            self.disconnect(translations::DISCONNECT_TIMEOUT.msg());
        }
    }

    /// Returns the current latency in milliseconds.
    /// This is a smoothed average calculated from keep-alive round-trip times.
    #[must_use]
    pub fn latency(&self) -> i32 {
        *self.latency.lock() as i32
    }

    /// Disconnects the client.
    pub fn disconnect(&self, reason: impl Into<TextComponent>) {
        let packet = match EncodedPacket::from_bare(
            CDisconnect::new(&reason.into(), self),
            self.compression,
            ConnectionProtocol::Play,
        ) {
            Ok(packet) => packet,
            Err(err) => {
                log::warn!(
                    "Failed to encode disconnect packet for client {}: {err}",
                    self.id
                );
                self.close();
                return;
            }
        };
        if self
            .outgoing_packets
            .send(OutboundPacket::Disconnect(packet))
            .is_err()
        {
            self.close();
            return;
        }
        self.close();
    }

    /// Sends a packet to the client.
    ///
    /// # Panics
    /// - If the packet fails to be encoded.
    /// - If the packet fails to be sent through the channel.
    pub fn send_packet<P: ClientPacket>(&self, packet: P) {
        let packet = EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
            .expect("Failed to encode packet");
        if self
            .outgoing_packets
            .send(OutboundPacket::Packet(packet))
            .is_err()
        {
            self.close();
        }
    }

    /// Sends an encoded packet to the client.
    ///
    /// # Panics
    /// - If the packet fails to be sent through the channel.
    pub fn send_encoded_packet(&self, packet: EncodedPacket) {
        if self
            .outgoing_packets
            .send(OutboundPacket::Packet(packet))
            .is_err()
        {
            self.close();
        }
    }

    /// Closes the connection.
    pub fn close(&self) {
        self.cancel_token.cancel();
    }

    /// Returns whether the connection is closed.
    #[must_use]
    pub fn closed(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    /// Waits for the connection to be closed.
    pub async fn wait_for_close(&self) {
        self.cancel_token.cancelled().await;
    }

    fn can_process_before_join(packet_id: i32) -> bool {
        match packet_id {
            play::S_KEEP_ALIVE | play::S_PING_REQUEST | play::S_CHUNK_BATCH_RECEIVED => true,
            _ => play_packet_descriptor(packet_id)
                .is_some_and(|descriptor| descriptor.can_process_before_join),
        }
    }

    fn can_process_during_domain_handshake(packet_id: i32) -> bool {
        match packet_id {
            play::S_CHUNK_BATCH_RECEIVED => true,
            _ => play_packet_descriptor(packet_id)
                .is_some_and(|descriptor| descriptor.can_process_during_domain_handshake),
        }
    }

    /// Decodes and dispatches one packet received from the client.
    fn process_packet(
        &self,
        packet: RawPacket,
        player: Arc<Player>,
        server: &Server,
    ) -> Result<(), PacketError> {
        if !player.has_joined_world() && !Self::can_process_before_join(packet.id) {
            return Ok(());
        }

        let payload_bytes = packet.payload().len();
        let Some(packet) = Self::decode_domain_gated_packet(packet, &player)? else {
            return Ok(());
        };

        match packet {
            DecodedPlayPacket::Scheduled(packet) => {
                server.schedule_play_packet(player, packet, payload_bytes);
            }
            DecodedPlayPacket::Immediate(packet) => {
                self.handle_immediate_packet(packet, &player);
            }
        }
        Ok(())
    }

    fn decode_domain_gated_packet(
        packet: RawPacket,
        player: &Player,
    ) -> Result<Option<DecodedPlayPacket>, PacketError> {
        let maintenance_packet = matches!(packet.id, play::S_KEEP_ALIVE | play::S_PING_REQUEST);
        let handshake_packet = Self::can_process_during_domain_handshake(packet.id);
        if packet.id == play::S_CLIENT_COMMAND {
            let decoded = Self::decode_play_packet(packet)?;
            let perform_respawn = matches!(
                &decoded,
                DecodedPlayPacket::Scheduled(packet) if packet.is_perform_respawn()
            );
            if player.gate_domain_switch_packet(false, perform_respawn) {
                return Ok(Some(decoded));
            }
            return Ok(None);
        }

        if !maintenance_packet && !player.gate_domain_switch_packet(handshake_packet, false) {
            return Ok(None);
        }

        Self::decode_play_packet(packet).map(Some)
    }

    fn decode_play_packet(packet: RawPacket) -> Result<DecodedPlayPacket, PacketError> {
        // Immediate packets bypass the inter-tick scheduling phase entirely.
        let data = &mut Cursor::new(packet.payload());
        let immediate = match packet.id {
            play::S_KEEP_ALIVE => ImmediatePlayPacket::KeepAlive(SKeepAlive::read_packet(data)?),
            play::S_PING_REQUEST => {
                ImmediatePlayPacket::PingRequest(SPingRequest::read_packet(data)?)
            }
            play::S_CHUNK_BATCH_RECEIVED => {
                ImmediatePlayPacket::ChunkBatchReceived(SChunkBatchReceived::read_packet(data)?)
            }
            _ => match play_packet_descriptor(packet.id) {
                Some(descriptor) => {
                    let kind = (descriptor.decode)(data)?;
                    return Ok(DecodedPlayPacket::Scheduled(ScheduledPlayPacket::new(
                        descriptor, kind,
                    )));
                }
                None => ImmediatePlayPacket::Unknown(packet.id),
            },
        };

        Ok(DecodedPlayPacket::Immediate(immediate))
    }

    fn handle_immediate_packet(&self, packet: ImmediatePlayPacket, player: &Player) {
        match packet {
            ImmediatePlayPacket::KeepAlive(packet) => self.handle_keep_alive(packet),
            ImmediatePlayPacket::PingRequest(packet) => {
                player.send_packet(CPongResponse::new(packet.time));
            }
            ImmediatePlayPacket::ChunkBatchReceived(packet) => {
                player
                    .chunk_sender
                    .lock()
                    .on_chunk_batch_received_by_client(packet.desired_chunks_per_tick);
            }
            ImmediatePlayPacket::Unknown(id) => log::info!("play packet id {id} is not known"),
        }
    }

    /// Listens for packets from the client.
    pub async fn listener(
        &self,
        mut reader: TCPNetworkDecoder<BufReader<OwnedReadHalf>>,
        server: Arc<Server>,
    ) {
        loop {
            select! {
                () = self.wait_for_close() => {
                    break;
                }
                packet = reader.get_raw_packet() => {
                    match packet {
                        Ok(packet) => {
                            if let Some(player) = self.player.upgrade()
                                && let Err(err) = self.process_packet(packet, player, &server) {
                                log::warn!(
                                    "Failed to get packet from client {}: {err}",
                                    self.id
                                );
                            }
                        }
                        Err(err) => {
                            log::debug!("Failed to get raw packet from client {}: {err}", self.id);
                            self.close();
                        }
                    }
                }
            }
        }
    }

    /// Sends packets to the client.
    ///
    pub async fn sender(&self, mut sender_recv: UnboundedReceiver<OutboundPacket>) {
        let disconnect_packet = loop {
            select! {
                biased;
                () = self.wait_for_close() => {
                    break Self::take_queued_disconnect(&mut sender_recv);
                }
                outbound = sender_recv.recv() => {
                    if let Some(outbound) = outbound {
                        let (packet, close_after_write) = match outbound {
                            OutboundPacket::Packet(packet) => (packet, false),
                            OutboundPacket::Disconnect(packet) => (packet, true),
                        };

                        if close_after_write {
                            self.close();
                            break Some(packet);
                        }

                        let write_result = self.write_packet_now(&packet);
                        select! {
                            biased;
                            () = self.wait_for_close() => {
                                break Self::take_queued_disconnect(&mut sender_recv);
                            },
                            result = write_result => {
                                if let Err(err) = result {
                                    log::warn!("Failed to send packet to client {}: {err}", self.id);
                                    self.close();
                                    break None;
                                }
                            }
                        }
                    } else {
                        //log::warn!(
                        //    "Internal packet_sender_recv channel closed for client {}",
                        //    self.id
                        //);
                        self.close();
                        break None;
                    }
                }
            }
        };

        self.finish_disconnect(disconnect_packet).await;
    }

    fn take_queued_disconnect(
        sender_recv: &mut UnboundedReceiver<OutboundPacket>,
    ) -> Option<EncodedPacket> {
        let mut disconnect_packet = None;
        loop {
            match sender_recv.try_recv() {
                Ok(OutboundPacket::Packet(_)) => {}
                Ok(OutboundPacket::Disconnect(packet)) => disconnect_packet = Some(packet),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        disconnect_packet
    }
}

impl TextResolutor for JavaConnection {
    fn resolve_content(&self, _resolvable: &Resolvable) -> TextComponent {
        TextComponent::new()
    }

    fn resolve_custom(&self, _data: &CustomData) -> Option<TextComponent> {
        None
    }

    fn translate(&self, _key: &str) -> Option<String> {
        None
    }
}

impl NetworkConnection for JavaConnection {
    fn compression(&self) -> Option<CompressionInfo> {
        self.compression
    }

    fn send_encoded(&self, packet: EncodedPacket) {
        self.send_encoded_packet(packet);
    }

    fn send_encoded_bundle(&self, packets: Vec<EncodedPacket>) {
        self.send_packet(CBundleDelimiter);
        for packet in packets {
            self.send_encoded_packet(packet);
        }
        self.send_packet(CBundleDelimiter);
    }

    fn disconnect_with_reason(&self, reason: TextComponent) {
        self.disconnect(reason);
    }

    fn tick(&self) {
        self.keep_connection_alive();
    }

    fn latency(&self) -> i32 {
        *self.latency.lock() as i32
    }

    fn close(&self) {
        JavaConnection::close(self);
    }

    fn closed(&self) -> bool {
        self.cancel_token.is_cancelled()
    }
}

#[cfg(test)]
mod tests {
    use std::array;

    use crate::{
        entity::{Entity as _, LivingEntity as _},
        test_support::{TestPlayerBuilder, fresh_test_world},
    };
    use rustc_hash::FxHashMap;
    use steel_protocol::packets::common::{ChatVisibility, HumanoidArm, ParticleStatus};
    use steel_protocol::packets::game::{ClickType, ClientCommandAction, HashedStack};
    use steel_registry::{blocks::properties::Direction, item_stack::ItemStack};
    use steel_utils::{BlockPos, codec::VarInt, types::InteractionHand};
    use uuid::Uuid;

    use super::*;

    fn decode(packet: RawPacket) -> DecodedPlayPacket {
        let Ok(decoded) = JavaConnection::decode_play_packet(packet) else {
            panic!("test play packet should decode");
        };
        decoded
    }

    fn execution(
        descriptor: &'static PlayPacketDescriptor,
        kind: ScheduledPlayPacketKind,
    ) -> ScheduledPacketExecution {
        ScheduledPlayPacket::new(descriptor, kind).execution()
    }

    #[test]
    fn pre_join_custom_payload_uses_serverbound_play_packet_id() {
        assert!(JavaConnection::can_process_before_join(
            play::S_CUSTOM_PAYLOAD
        ));
        assert!(!JavaConnection::can_process_before_join(
            play::C_CUSTOM_PAYLOAD
        ));
    }

    #[test]
    fn queued_domain_switch_records_only_perform_respawn_at_connection_gate() {
        let world = fresh_test_world("queued_domain_switch_respawn_packet");
        let player = TestPlayerBuilder::new(world, "RespawnTester", 1).build();
        let Some(token) = player.begin_pending_world_change() else {
            panic!("test player should acquire a world-change token");
        };
        assert!(player.begin_domain_switch(token));
        player.set_health(0.0);

        let request_stats = JavaConnection::decode_domain_gated_packet(
            RawPacket::new(
                play::S_CLIENT_COMMAND,
                vec![ClientCommandAction::RequestStats as u8],
            ),
            &player,
        );
        assert!(matches!(request_stats, Ok(None)));
        assert!(!player.has_deferred_death_respawn_for_test());

        let perform_respawn = JavaConnection::decode_domain_gated_packet(
            RawPacket::new(
                play::S_CLIENT_COMMAND,
                vec![ClientCommandAction::PerformRespawn as u8],
            ),
            &player,
        );
        assert!(matches!(perform_respawn, Ok(None)));
        assert!(player.has_deferred_death_respawn_for_test());

        assert!(player.finish_domain_switch(token));
        assert!(player.finish_pending_world_change(token));
    }

    #[test]
    fn custom_payload_defaults_to_global_exclusive_scheduling() {
        let channel = b"minecraft:brand";
        let mut payload = vec![channel.len() as u8];
        payload.extend_from_slice(channel);
        payload.extend_from_slice(b"steel");
        let decoded = decode(RawPacket::new(play::S_CUSTOM_PAYLOAD, payload));
        let DecodedPlayPacket::Scheduled(
            packet @ ScheduledPlayPacket {
                kind: ScheduledPlayPacketKind::CustomPayload(_),
                ..
            },
        ) = decoded
        else {
            panic!("custom payload should use the scheduled packet path");
        };

        assert_eq!(packet.execution(), ScheduledPacketExecution::Exclusive);
    }

    #[test]
    fn pre_join_allows_initial_play_acknowledgements() {
        assert!(JavaConnection::can_process_before_join(
            play::S_ACCEPT_TELEPORTATION
        ));
        assert!(JavaConnection::can_process_before_join(
            play::S_CHUNK_BATCH_RECEIVED
        ));
        assert!(JavaConnection::can_process_before_join(
            play::S_PLAYER_LOADED
        ));
        assert!(JavaConnection::can_process_during_domain_handshake(
            play::S_ACCEPT_TELEPORTATION
        ));
        assert!(JavaConnection::can_process_during_domain_handshake(
            play::S_CHUNK_BATCH_RECEIVED
        ));
        assert!(JavaConnection::can_process_during_domain_handshake(
            play::S_PLAYER_LOADED
        ));
        assert!(!JavaConnection::can_process_during_domain_handshake(
            play::S_MOVE_PLAYER_POS
        ));
    }

    #[test]
    fn scheduled_domain_handshake_classification_is_narrow() {
        let accept = decode(RawPacket::new(play::S_ACCEPT_TELEPORTATION, vec![0]));
        let DecodedPlayPacket::Scheduled(accept) = accept else {
            panic!("teleport acknowledgement should be scheduled");
        };
        assert!(accept.is_domain_handshake_packet());

        let client_tick_end = decode(RawPacket::new(play::S_CLIENT_TICK_END, Vec::new()));
        let DecodedPlayPacket::Scheduled(client_tick_end) = client_tick_end else {
            panic!("client tick end should be scheduled");
        };
        assert!(!client_tick_end.is_domain_handshake_packet());
    }

    #[test]
    fn client_tick_end_is_scheduled_for_the_inter_tick_phase() {
        let decoded = decode(RawPacket::new(play::S_CLIENT_TICK_END, Vec::new()));

        assert!(matches!(
            decoded,
            DecodedPlayPacket::Scheduled(ScheduledPlayPacket {
                kind: ScheduledPlayPacketKind::ClientTickEnd,
                ..
            })
        ));
    }

    #[test]
    fn packet_execution_classification_separates_local_and_serialized_work() {
        assert_eq!(
            execution(
                &PLAYER_ABILITIES_PACKET,
                ScheduledPlayPacketKind::PlayerAbilities(SPlayerAbilities { flags: 0 },)
            ),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            execution(
                &MOVE_PLAYER_STATUS_ONLY_PACKET,
                ScheduledPlayPacketKind::MovePlayer(
                    SMovePlayerStatusOnly { packed_byte: 0 }.into(),
                )
            ),
            ScheduledPacketExecution::Serialized
        );
    }

    #[test]
    fn inventory_execution_reflects_complete_transaction_boundaries() {
        let click = SContainerClick {
            container_id: 0,
            state_id: 0,
            slot_num: 0,
            button_num: 0,
            click_type: ClickType::Pickup,
            changed_slots: FxHashMap::default(),
            carried_item: HashedStack::Empty,
        };

        assert_eq!(
            execution(
                &CONTAINER_CLICK_PACKET,
                ScheduledPlayPacketKind::ContainerClick(click)
            ),
            ScheduledPacketExecution::Serialized
        );
        assert_eq!(
            execution(
                &CONTAINER_CLOSE_PACKET,
                ScheduledPlayPacketKind::ContainerClose(SContainerClose { container_id: 0 })
            ),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            execution(
                &SET_CREATIVE_MODE_SLOT_PACKET,
                ScheduledPlayPacketKind::SetCreativeModeSlot(SSetCreativeModeSlot {
                    slot_num: 1,
                    item_stack: ItemStack::empty(),
                },)
            ),
            ScheduledPacketExecution::PlayerLocal
        );
    }

    #[test]
    fn player_command_execution_is_action_sensitive() {
        let command = |action| {
            execution(
                &PLAYER_COMMAND_PACKET,
                ScheduledPlayPacketKind::PlayerCommand(SPlayerCommand {
                    entity_id: 1,
                    action,
                    data: 0,
                }),
            )
        };

        assert_eq!(
            command(PlayerCommandAction::StartSprinting),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            command(PlayerCommandAction::StartFallFlying),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            command(PlayerCommandAction::LeaveBed),
            ScheduledPacketExecution::Serialized
        );
        assert_eq!(
            command(PlayerCommandAction::OpenVehicleInventory),
            ScheduledPacketExecution::Exclusive
        );
    }

    #[test]
    fn player_action_execution_is_action_sensitive() {
        let action = |action| {
            execution(
                &PLAYER_ACTION_PACKET,
                ScheduledPlayPacketKind::PlayerAction(SPlayerAction {
                    action,
                    pos: BlockPos::new(0, 64, 0),
                    direction: Direction::Down,
                    sequence: 0,
                }),
            )
        };

        assert_eq!(
            action(PlayerAction::AbortDestroyBlock),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            action(PlayerAction::SwapItemWithOffhand),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            action(PlayerAction::StartDestroyBlock),
            ScheduledPacketExecution::Serialized
        );
        assert_eq!(
            action(PlayerAction::Stab),
            ScheduledPacketExecution::Exclusive
        );
    }

    #[test]
    fn chat_message_and_ack_share_the_serialized_commit_lane() {
        assert_eq!(
            execution(
                &CHAT_PACKET,
                ScheduledPlayPacketKind::Chat(Box::new(SChat {
                    message: "hello".to_owned(),
                    timestamp: 0,
                    salt: 0,
                    signature: None,
                    offset: 0,
                    acknowledged: [0; 3],
                    checksum: 0,
                }))
            ),
            ScheduledPacketExecution::Serialized
        );
        assert_eq!(
            execution(
                &CHAT_ACK_PACKET,
                ScheduledPlayPacketKind::ChatAck(SChatAck { offset: VarInt(0) })
            ),
            ScheduledPacketExecution::Serialized
        );
    }

    #[test]
    fn cross_player_and_unimplemented_handlers_remain_global_barriers() {
        assert_eq!(
            execution(
                &ATTACK_PACKET,
                ScheduledPlayPacketKind::Attack(SAttack { entity_id: 1 })
            ),
            ScheduledPacketExecution::Exclusive
        );
        assert_eq!(
            execution(
                &CONTAINER_BUTTON_CLICK_PACKET,
                ScheduledPlayPacketKind::ContainerButtonClick(SContainerButtonClick {
                    container_id: 1,
                    button_id: 0,
                }),
            ),
            ScheduledPacketExecution::Exclusive
        );
        assert_eq!(
            execution(
                &PLAYER_ACTION_PACKET,
                ScheduledPlayPacketKind::PlayerAction(SPlayerAction {
                    action: PlayerAction::ReleaseUseItem,
                    pos: BlockPos::new(0, 64, 0),
                    direction: Direction::Down,
                    sequence: 0,
                })
            ),
            ScheduledPacketExecution::Exclusive
        );
    }

    #[test]
    fn audited_handlers_use_the_narrowest_safe_execution_class() {
        assert_eq!(
            execution(
                &ACCEPT_TELEPORTATION_PACKET,
                ScheduledPlayPacketKind::AcceptTeleportation(SAcceptTeleportation {
                    teleport_id: 1
                },)
            ),
            ScheduledPacketExecution::Serialized
        );
        assert_eq!(
            execution(
                &PLAYER_INPUT_PACKET,
                ScheduledPlayPacketKind::PlayerInput(SPlayerInput { flags: 0 })
            ),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            execution(
                &CHAT_SESSION_UPDATE_PACKET,
                ScheduledPlayPacketKind::ChatSessionUpdate(SChatSessionUpdate {
                    session_id: Uuid::nil(),
                    expires_at: 0,
                    public_key: Vec::new(),
                    key_signature: Vec::new(),
                },)
            ),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            execution(
                &CLIENT_INFORMATION_PACKET,
                ScheduledPlayPacketKind::ClientInformation(SClientInformation {
                    language: "en_us".to_owned(),
                    view_distance: 8,
                    chat_visibility: ChatVisibility::Full,
                    chat_colors: true,
                    model_customization: 0,
                    main_hand: HumanoidArm::Right,
                    text_filtering_enabled: false,
                    allows_listing: true,
                    particle_status: ParticleStatus::All,
                },)
            ),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            execution(
                &CHAT_COMMAND_PACKET,
                ScheduledPlayPacketKind::ChatCommand(SChatCommand {
                    command: "help".to_owned(),
                })
            ),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            execution(
                &PICK_ITEM_FROM_BLOCK_PACKET,
                ScheduledPlayPacketKind::PickItemFromBlock(SPickItemFromBlock {
                    pos: BlockPos::new(0, 64, 0),
                    include_data: false,
                },)
            ),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            execution(
                &SIGN_UPDATE_PACKET,
                ScheduledPlayPacketKind::SignUpdate(SSignUpdate {
                    pos: BlockPos::new(0, 64, 0),
                    is_front_text: true,
                    lines: array::from_fn(|_| String::new()),
                })
            ),
            ScheduledPacketExecution::Serialized
        );
        assert_eq!(
            execution(
                &SWING_PACKET,
                ScheduledPlayPacketKind::Swing(SSwing {
                    hand: InteractionHand::MainHand,
                })
            ),
            ScheduledPacketExecution::PlayerLocal
        );
        assert_eq!(
            execution(
                &CLIENT_COMMAND_PACKET,
                ScheduledPlayPacketKind::ClientCommand(SClientCommand {
                    action: ClientCommandAction::PerformRespawn,
                })
            ),
            ScheduledPacketExecution::PlayerLocal
        );
    }

    #[test]
    fn keep_alive_remains_on_the_immediate_connection_path() {
        let decoded = decode(RawPacket::new(
            play::S_KEEP_ALIVE,
            42_i64.to_be_bytes().to_vec(),
        ));

        assert!(matches!(
            decoded,
            DecodedPlayPacket::Immediate(ImmediatePlayPacket::KeepAlive(SKeepAlive { id: 42 }))
        ));
    }

    #[test]
    fn chunk_batch_ack_uses_the_immediate_connection_path() {
        let decoded = decode(RawPacket::new(
            play::S_CHUNK_BATCH_RECEIVED,
            12.5_f32.to_be_bytes().to_vec(),
        ));

        assert!(matches!(
            decoded,
            DecodedPlayPacket::Immediate(ImmediatePlayPacket::ChunkBatchReceived(
                SChunkBatchReceived {
                    desired_chunks_per_tick: 12.5
                }
            ))
        ));
    }
}
