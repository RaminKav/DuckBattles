use std::time::Duration;

use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::*,
};
use bevy_renet2::prelude::{ChannelConfig, ClientId, ConnectionConfig, SendType};
use serde::{Deserialize, Serialize};

// Client-Server setup stuff, move somewhere else later
// #[cfg(feature = "netcode")]
pub const PRIVATE_KEY: &[u8; bevy_renet2::netcode::NETCODE_KEY_BYTES] =
    b"an example very very secret key."; // 32-bytes
                                         // #[cfg(feature = "netcode")]
pub const PROTOCOL_ID: u64 = 7;

/// Preset duck tints (slot 0 = natural yellow / no tint).
pub const PLAYER_COLOR_COUNT: usize = 8;
/// Max concurrent players (matches spawn slots / color count).
pub const MAX_PLAYERS: usize = PLAYER_COLOR_COUNT;

/// Public lobby/match snapshot served at `GET /status` for the main menu.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicServerStatus {
    pub phase: ServerPhase,
    pub players: usize,
    pub max_players: usize,
    pub remaining_secs: Option<u16>,
}

impl Default for PublicServerStatus {
    fn default() -> Self {
        Self {
            phase: ServerPhase::Lobby,
            players: 0,
            max_players: MAX_PLAYERS,
            remaining_secs: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerPhase {
    Lobby,
    Match,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default, Reflect)]
#[reflect(Component)]
pub struct Player {
    pub id: ClientId,
    pub score: i64,
    pub is_ready: bool,
    /// Index into [`player_color_name`] / [`player_color_tint`] (0..=7).
    pub color: u8,
}

pub fn player_color_name(slot: u8) -> &'static str {
    match slot % PLAYER_COLOR_COUNT as u8 {
        0 => "Yellow",
        1 => "Red",
        2 => "Blue",
        3 => "Green",
        4 => "Orange",
        5 => "Purple",
        6 => "Pink",
        _ => "Cyan",
    }
}

pub fn player_color_tint(slot: u8) -> Color {
    match slot % PLAYER_COLOR_COUNT as u8 {
        0 => Color::WHITE, // no tint — base duck yellow
        1 => Color::srgb(1.0, 0.25, 0.25),
        2 => Color::srgb(0.25, 0.45, 1.0),
        3 => Color::srgb(0.25, 0.9, 0.35),
        4 => Color::srgb(1.0, 0.55, 0.1),
        5 => Color::srgb(0.7, 0.3, 1.0),
        6 => Color::srgb(1.0, 0.45, 0.75),
        _ => Color::srgb(0.2, 0.9, 0.95),
    }
}

/// UI-friendly color (Yellow uses a visible gold instead of white).
pub fn player_color_label(slot: u8) -> Color {
    match slot % PLAYER_COLOR_COUNT as u8 {
        0 => Color::srgb(0.95, 0.85, 0.2),
        other => player_color_tint(other),
    }
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, Component, Resource)]
pub struct PlayerInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}

#[derive(Debug, Serialize, Deserialize, Event)]
pub enum PlayerCommand {
    /// Fire toward a world-space aim point (from the client's cursor).
    BasicAttack {
        aim: [f32; 2],
    },
    /// Short burst dash in the player's current move/facing direction.
    Dash,
    ToggleReady,
    DebugSpawnBot,
    /// Ask the server to reset the match and disconnect everyone to the title screen.
    ResetServer,
}
pub enum ClientChannel {
    Input,
    Command,
}
pub enum ServerChannel {
    ServerMessages,
    NetworkedEntities,
}

#[derive(Debug, Default, Component)]
pub struct Velocity(pub Vec3);

#[derive(Debug, Serialize, Deserialize, Component)]
pub enum ServerMessages {
    PlayerCreate {
        entity: Entity,
        id: ClientId,
        translation: [f32; 3],
        is_ready: bool,
        color: u8,
    },
    SpawnGameObject {
        id: u64,
        translation: [f32; 3],
    },
    PlayerRemove {
        id: ClientId,
    },
    SpawnProjectile {
        entity: Entity,
        translation: [f32; 3],
        angle: f32,
    },
    SpawnCoin {
        entity: Entity,
        translation: [f32; 3],
    },
    DespawnEntity {
        entity: Entity,
    },
    SetPlayerReady {
        entity: Entity,
        is_ready: bool,
    },
    StartGame,
    /// Server is resetting; clients should return to the title screen.
    ReturnToTitle,
    /// Authoritative match countdown (whole seconds remaining).
    MatchTimer {
        remaining_secs: u16,
    },
    /// Match over — show leaderboard then return to menu.
    MatchEnded {
        rankings: Vec<LeaderboardEntry>,
    },
    /// A player was hit by a projectile (for client VFX / "HIT!" text).
    PlayerHit {
        entity: Entity,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderboardEntry {
    pub client_id: ClientId,
    pub score: i64,
    pub color: u8,
}

/// Cleared on the next frame: drops the Renet client session after leaving a match.
#[derive(Resource)]
pub struct PendingSessionTeardown;

/// Authoritative post-match rankings for the results screen.
#[derive(Resource, Clone, Debug)]
pub struct MatchResults {
    pub rankings: Vec<LeaderboardEntry>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct NetworkedEntities {
    pub entities: Vec<Entity>,
    pub translations: Vec<[f32; 3]>,
    pub facing_directions: Vec<Option<[f32; 2]>>,
    pub score: Vec<Option<i64>>,
}

impl From<ClientChannel> for u8 {
    fn from(channel_id: ClientChannel) -> Self {
        match channel_id {
            ClientChannel::Command => 0,
            ClientChannel::Input => 1,
        }
    }
}

impl ClientChannel {
    pub fn channels_config() -> Vec<ChannelConfig> {
        vec![
            ChannelConfig {
                channel_id: Self::Input.into(),
                max_memory_usage_bytes: 1000 * 1024 * 1024,
                send_type: SendType::ReliableOrdered {
                    resend_time: Duration::ZERO,
                },
            },
            ChannelConfig {
                channel_id: Self::Command.into(),
                max_memory_usage_bytes: 1000 * 1024 * 1024,
                send_type: SendType::ReliableOrdered {
                    resend_time: Duration::ZERO,
                },
            },
        ]
    }
}

impl From<ServerChannel> for u8 {
    fn from(channel_id: ServerChannel) -> Self {
        match channel_id {
            ServerChannel::NetworkedEntities => 0,
            ServerChannel::ServerMessages => 1,
        }
    }
}

impl ServerChannel {
    pub fn channels_config() -> Vec<ChannelConfig> {
        vec![
            ChannelConfig {
                channel_id: Self::NetworkedEntities.into(),
                max_memory_usage_bytes: 1000 * 1024 * 1024,
                send_type: SendType::Unreliable,
            },
            ChannelConfig {
                channel_id: Self::ServerMessages.into(),
                max_memory_usage_bytes: 1000 * 1024 * 1024,
                send_type: SendType::ReliableOrdered {
                    resend_time: Duration::from_millis(200),
                },
            },
        ]
    }
}

pub fn connection_config() -> ConnectionConfig {
    ConnectionConfig {
        available_bytes_per_tick: 1024 * 1024,
        client_channels_config: ClientChannel::channels_config(),
        server_channels_config: ServerChannel::channels_config(),
    }
}
