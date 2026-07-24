use std::collections::HashMap;
#[cfg(not(target_family = "wasm"))]
use std::time::{SystemTime, UNIX_EPOCH};

use crate::demo::animation::{FacingDirection, PlayerAnimation};

use crate::demo::lib::connection_config;
use crate::demo::physics::Collider;
use crate::screens::gameplay::{
    calculate_score_growth, COIN_COLLIDER_SIZE, COIN_SCALE, ScoreText,
};
use crate::screens::lobby::ToggleReadyEvent;
use crate::screens::Screen;
use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::Vec3,
    prelude::*,
};
use bevy_mod_reqwest::*;
use bevy_renet2::netcode::NetcodeClientPlugin;
use renet2_netcode::NetcodeClientTransport;

use bevy_renet2::prelude::{
    client_connected, client_just_disconnected, ClientId, RenetClient, RenetClientPlugin,
};
use renet2_setup::{setup_renet2_client, ClientConnectPack, ConnectionType, ServerConnectToken};

use super::lib::{
    player_color_tint, ClientChannel, MatchResults, NetworkedEntities, PendingSessionTeardown,
    Player, PlayerCommand, PlayerInput, ServerChannel, ServerMessages, PROTOCOL_ID,
};
use super::player::PlayerAssets;

#[derive(Component)]
pub struct ControlledPlayer;

#[derive(Default, Resource)]
pub struct NetworkMapping(pub HashMap<Entity, Entity>);

#[derive(Debug, Default, Resource)]
pub struct ClientLobby {
    pub players: HashMap<ClientId, PlayerInfo>,
}

#[derive(Debug)]
pub struct PlayerInfo {
    pub client_entity: Entity,
    pub server_entity: Entity,
}

#[derive(Debug, Resource)]
pub struct CurrentClientId(u64);

#[derive(Resource, Clone, Debug)]
pub struct ClientNetworkConfig {
    pub auth_base_url: String,
    pub connection_type: ConnectionType,
}

impl ClientNetworkConfig {
    fn resolve_auth_base_url() -> String {
        std::env::var("CHEXY_AUTH_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| option_env!("CHEXY_AUTH_BASE_URL").map(|value| value.to_string()))
            .unwrap_or_else(|| "http://localhost:8080".to_string())
    }

    fn resolve_connection_type() -> ConnectionType {
        fn parse(value: &str) -> Option<ConnectionType> {
            match value.to_ascii_lowercase().as_str() {
                "memory" => Some(ConnectionType::Memory),
                "native" => Some(ConnectionType::Native),
                "wasm_wt" | "wasm-wt" => Some(ConnectionType::WasmWt),
                "wasm_ws" | "wasm-ws" => Some(ConnectionType::WasmWs),
                _ => None,
            }
        }

        let env_value = std::env::var("CHEXY_CLIENT_TRANSPORT")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| option_env!("CHEXY_CLIENT_TRANSPORT").map(|value| value.to_string()));

        env_value
            .and_then(|value| parse(value.trim()))
            .unwrap_or_else(ConnectionType::inferred)
    }

    fn transport_slug(connection_type: ConnectionType) -> &'static str {
        match connection_type {
            ConnectionType::Memory => "memory",
            ConnectionType::Native => "native",
            ConnectionType::WasmWt => "wasm_wt",
            ConnectionType::WasmWs => "wasm_ws",
        }
    }

    pub fn from_env() -> Self {
        Self {
            auth_base_url: Self::resolve_auth_base_url(),
            connection_type: Self::resolve_connection_type(),
        }
    }

    fn auth_endpoint(&self, client_id: u64) -> String {
        format!(
            "{}/auth/{client_id}?transport={}",
            self.auth_base_url.trim_end_matches('/'),
            Self::transport_slug(self.connection_type)
        )
    }

    pub fn status_endpoint(&self) -> String {
        format!("{}/status", self.auth_base_url.trim_end_matches('/'))
    }
}

impl Default for ClientNetworkConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Connected;

#[cfg(target_family = "wasm")]
fn now_millis() -> u64 {
    js_sys::Date::now() as u64
}

#[cfg(not(target_family = "wasm"))]
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub const PLAYER_BASE_COLLIDER_SIZE: Vec2 = Vec2::new(14., 10.);
pub fn client_connected2(client: Option<Res<RenetClient>>) -> bool {
    let c = client_connected(client);
    c
}

pub fn setup_client_fr(mut client: BevyReqwest, config: Res<ClientNetworkConfig>) {
    let client_id = now_millis();
    let url = config.auth_endpoint(client_id);
    println!("Setting up initial connection to server...");
    bevy::log::info!(
        "[CLIENT] Connecting to server at {url} using {:?}",
        config.connection_type
    );
    let reqwest_request = client.post(url).build().expect("Failed to build request");
    client
        .send(reqwest_request)
        .on_json_response(
            move |trigger: Trigger<JsonResponse<ServerConnectToken>>, mut commands: Commands| {
                let token = trigger.event().0.clone();
                bevy::log::info!("[CLIENT] got response from server: {token:?}");

                let connect_pack = ClientConnectPack::new(PROTOCOL_ID, token)
                    .expect("Failed to create connect pack");
                let connection_config = connection_config();

                commands.remove_resource::<NetcodeClientTransport>();
                let (client, transport) = setup_renet2_client(connection_config, connect_pack)
                    .expect("Failed to setup renet2 client");

                commands.insert_resource(client);
                commands.insert_resource(transport);
                commands.insert_resource(CurrentClientId(client_id));
                bevy::log::info!("[CLIENT] DONE setting up connection with server!!");
            },
        )
        .on_error(|trigger: Trigger<ReqwestErrorEvent>| {
            let e = &trigger.event().0;
            bevy::log::info!("error: {e:?}");
        });
}
// #[cfg(feature = "netcode")]
fn add_netcode_network(app: &mut App) {
    use super::lib::PROTOCOL_ID;
    use bevy_renet2::netcode::{
        ClientAuthentication, NetcodeClientPlugin, NetcodeClientTransport, NetcodeTransportError,
    };
    use std::{net::UdpSocket, time::SystemTime};

    app.add_plugins(NetcodeClientPlugin);

    app.configure_sets(Update, Connected.run_if(client_connected2));

    // If any error is found we just panic
    #[allow(clippy::never_loop)]
    fn panic_on_error_system(mut renet_error: EventReader<NetcodeTransportError>) {
        for e in renet_error.read() {
            panic!("{}", e);
        }
    }

    // #[cfg(target_family = "wasm")]
    // fn connect_wasm(mut client: BevyReqwest, mut commands: Commands) {
    //     use renet2_netcode::{
    //         webtransport_is_available_with_cert_hashes, ClientSocket, CongestionControl, NetcodeClientTransport, ServerCertHash, WebServerDestination, WebSocketClient, WebSocketClientConfig, WebTransportClient, WebTransportClientConfig
    //     };

    //     let url = "https://bored-api.appbrewery.com/random";

    //     let reqwest_request = client.get(url).build().unwrap();

    //     client
    //         .send(reqwest_request)
    //         .on_json_response(
    //             |trigger: Trigger<
    //                 JsonResponse<(WebServerDestination, ServerCertHash, url::Url)>,
    //             >| {
    //                 let (wt_server_dest, wt_server_cert_hash, ws_server_url) = trigger.event().0;
    //                 let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    //     let connection_config = ConnectionConfig::test();
    //     let (client, transport, client_id) = match webtransport_is_available_with_cert_hashes() {
    //         true => {
    //             tracing::info!("setting up webtransport client (server = {:?})", wt_server_dest);

    //             let client_id = current_time.as_millis() as u64;
    //             let client_auth = ClientAuthentication::Unsecure {
    //                 client_id,
    //                 protocol_id: 0,
    //                 socket_id: 1, //WebTransport socket id is 1 in this example
    //                 server_addr: wt_server_dest.clone().into(),
    //                 user_data: None,
    //             };
    //             let socket_config = WebTransportClientConfig {
    //                 server_dest: wt_server_dest.into(),
    //                 congestion_control: CongestionControl::default(),
    //                 server_cert_hashes: Vec::from([wt_server_cert_hash]),
    //             };
    //             let socket = WebTransportClient::new(socket_config);

    //             let client = RenetClient::new(connection_config, socket.is_reliable());
    //             let transport = NetcodeClientTransport::new(current_time, client_auth, socket).unwrap();

    //             (client, transport, client_id)
    //         }
    //         false => {
    //             tracing::warn!("webtransport with cert hashes is not supported on this platform, falling back \
    //                 to websockets");
    //             tracing::info!("setting up websocket client (server = {:?})", ws_server_url.as_str());
    //             let socket_config = WebSocketClientConfig {
    //                 server_url: ws_server_url,
    //             };

    //             let socket = WebSocketClient::new(socket_config).unwrap();
    //             let client = RenetClient::new(connection_config, socket.is_reliable());
    //             let client_id = current_time.as_millis() as u64;

    //             let client_auth = ClientAuthentication::Unsecure {
    //                 client_id,
    //                 protocol_id: 0,
    //                 socket_id: 2, //WebSocket socket id is 2 in this example
    //                 server_addr: socket.server_address(),
    //                 user_data: None,
    //             };
    //             let transport = NetcodeClientTransport::new(current_time, client_auth, socket).unwrap();

    //             (client, transport, client_id)
    //         }
    //     };
    //     commands.insert_resource(transport);
    //     commands.insert_resource(client);

    //     commands.insert_resource(CurrentClientId(client_id));
    //             },
    //         )
    //         // In case of request error, it can be reached using an observersystem as well
    //         .on_error(|trigger: Trigger<ReqwestErrorEvent>| {
    //             let e = &trigger.event().0;
    //             bevy::log::info!("error: {e:?}");
    //         });
    // }
    // #[cfg(not(target_family = "wasm"))]
    // fn connect_udp(mut commands: Commands) {
    //     println!("[CLIENT] Connecting to server...");
    //     let server_addr = "159.203.58.28:8080".parse().unwrap();
    //     let socket = UdpSocket::bind("0.0.0.0:0").unwrap();

    //     let client = RenetClient::new(connection_config(), false);

    //     let current_time = SystemTime::now()
    //         .duration_since(SystemTime::UNIX_EPOCH)
    //         .unwrap();
    //     let client_id = current_time.as_millis() as u64;
    //     let authentication = ClientAuthentication::Unsecure {
    //         client_id,
    //         protocol_id: PROTOCOL_ID,
    //         socket_id: 0,
    //         server_addr,
    //         user_data: None,
    //     };

    //     let transport = NetcodeClientTransport::new(current_time, authentication, NativeSocket::new(socket).unwrap()).unwrap();
    //     commands.insert_resource(transport);
    //     commands.insert_resource(client);

    //     commands.insert_resource(CurrentClientId(client_id));
    //     println!("[CLIENT] Connected!");

    // }
    app.add_systems(Update, panic_on_error_system);

    // #[cfg(target_family = "wasm")]
    // app.add_systems(
    //     Update,
    //     connect_wasm.run_if(in_state(Screen::Lobby).and(run_once)),
    // );

    // #[cfg(not(target_family = "wasm"))]
    // app.add_systems(
    //     Update,
    //     connect_udp.run_if(in_state(Screen::Lobby).and(run_once)),
    // );
}

pub(super) fn plugins(app: &mut App) {
    app.add_plugins(RenetClientPlugin);
    app.add_plugins(FrameTimeDiagnosticsPlugin);
    app.add_plugins(LogDiagnosticsPlugin::default());
    app.add_plugins(NetcodeClientPlugin);

    // #[cfg(feature = "netcode")]
    // add_netcode_network(app);

    app.add_event::<PlayerCommand>();

    app.insert_resource(ClientLobby::default());
    app.insert_resource(PlayerInput::default());
    app.insert_resource(NetworkMapping::default());
    app.insert_resource(MatchTimeRemaining(60));
    app.insert_resource(CursorWorldPos::default());
    app.insert_resource(LocalDashCooldown::ready());
    app.insert_resource(LocalShootCooldown::ready());

    app.add_systems(
        Update,
        (
            update_cursor_world_pos,
            player_input,
            update_dash_cooldown_bar,
            update_hit_flash_text,
        )
            .run_if(in_state(Screen::Gameplay)),
    );
    app.add_systems(Update, (debug_player_input));
    app.add_systems(Update, (player_read_input).run_if(in_state(Screen::Lobby)));
    app.configure_sets(Update, Connected.run_if(client_connected2));
    app.add_systems(
        Update,
        (
            client_send_input,
            update_score_text,
            update_match_timer_text,
            client_send_player_commands,
            client_sync_players,
        )
            .in_set(Connected),
    );
    app.add_systems(Update, apply_pending_session_teardown);
    app.add_systems(
        Update,
        return_to_title_on_disconnect
            .run_if(client_just_disconnected)
            .run_if(in_state(Screen::Lobby).or(in_state(Screen::Gameplay))),
    );
    app.add_systems(OnEnter(Screen::Lobby), setup_client_fr);
    app.add_systems(OnEnter(Screen::Gameplay), spawn_match_timer_ui);
    app.add_systems(Update, attach_dash_cooldown_bar.run_if(in_state(Screen::Gameplay)));
}

/// Marker for client-side world props synced from the server (walls, trees, etc.).
#[derive(Component)]
pub(crate) struct ClientWorldObject;

#[derive(Resource, Default)]
pub struct MatchTimeRemaining(pub u16);

#[derive(Component)]
struct MatchTimerText;

/// World-space cursor position (Camera2d projection of the mouse).
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct CursorWorldPos(pub Vec2);

const DASH_COOLDOWN_SECS: f32 = 2.5;
const SHOOT_COOLDOWN_SECS: f32 = 0.25;

#[derive(Resource)]
struct LocalDashCooldown(Timer);

impl LocalDashCooldown {
    fn ready() -> Self {
        let mut timer = Timer::from_seconds(DASH_COOLDOWN_SECS, TimerMode::Once);
        timer.set_elapsed(timer.duration());
        Self(timer)
    }
}

#[derive(Resource)]
struct LocalShootCooldown(Timer);

impl LocalShootCooldown {
    fn ready() -> Self {
        let mut timer = Timer::from_seconds(SHOOT_COOLDOWN_SECS, TimerMode::Once);
        timer.set_elapsed(timer.duration());
        Self(timer)
    }
}

#[derive(Component)]
struct DashCooldownBarRoot;

#[derive(Component)]
struct DashCooldownBarFill;

#[derive(Component)]
struct HitFlashText {
    timer: Timer,
}

fn update_cursor_world_pos(
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    camera_q: Query<(&Camera, &GlobalTransform)>,
    mut cursor: ResMut<CursorWorldPos>,
) {
    let Ok(window) = windows.get_single() else {
        return;
    };
    let Some(screen_pos) = window.cursor_position() else {
        return;
    };
    let Ok((camera, camera_transform)) = camera_q.get_single() else {
        return;
    };
    if let Ok(world_pos) = camera.viewport_to_world_2d(camera_transform, screen_pos) {
        cursor.0 = world_pos;
    }
}

fn player_input(
    time: Res<Time>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mouse_input: Res<ButtonInput<MouseButton>>,
    cursor: Res<CursorWorldPos>,
    mut player_input: ResMut<PlayerInput>,
    mut player_commands: EventWriter<PlayerCommand>,
    mut dash_cd: ResMut<LocalDashCooldown>,
    mut shoot_cd: ResMut<LocalShootCooldown>,
) {
    dash_cd.0.tick(time.delta());
    shoot_cd.0.tick(time.delta());

    player_input.left =
        keyboard_input.pressed(KeyCode::KeyA) || keyboard_input.pressed(KeyCode::ArrowLeft);
    player_input.right =
        keyboard_input.pressed(KeyCode::KeyD) || keyboard_input.pressed(KeyCode::ArrowRight);
    player_input.up =
        keyboard_input.pressed(KeyCode::KeyW) || keyboard_input.pressed(KeyCode::ArrowUp);
    player_input.down =
        keyboard_input.pressed(KeyCode::KeyS) || keyboard_input.pressed(KeyCode::ArrowDown);

    if mouse_input.just_pressed(MouseButton::Left) && shoot_cd.0.finished() {
        player_commands.send(PlayerCommand::BasicAttack {
            aim: cursor.0.to_array(),
        });
        shoot_cd.0.reset();
    }

    if keyboard_input.just_pressed(KeyCode::Space) && dash_cd.0.finished() {
        player_commands.send(PlayerCommand::Dash);
        dash_cd.0.reset();
    }
}
fn debug_player_input(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut player_commands: EventWriter<PlayerCommand>,
) {
    if keyboard_input.just_pressed(KeyCode::KeyB) {
        player_commands.send(PlayerCommand::DebugSpawnBot);
    }
}
fn player_read_input(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut player_commands: EventWriter<PlayerCommand>,
) {
    if keyboard_input.just_pressed(KeyCode::Space) {
        bevy::log::info!("Space pressed");
        player_commands.send(PlayerCommand::ToggleReady);
    }
}

fn client_send_input(player_input: Res<PlayerInput>, mut client: ResMut<RenetClient>) {
    let input_message = bincode::serialize(&*player_input).unwrap();

    client.send_message(ClientChannel::Input, input_message);
}

fn client_send_player_commands(
    mut player_commands: EventReader<PlayerCommand>,
    mut client: ResMut<RenetClient>,
) {
    for command in player_commands.read() {
        bevy::log::info!("Sending command: {:?}", command);

        let command_message = bincode::serialize(command).unwrap();
        client.send_message(ClientChannel::Command, command_message);
    }
}

pub fn client_sync_players(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut client: ResMut<RenetClient>,
    client_id: Res<CurrentClientId>,
    mut lobby: ResMut<ClientLobby>,
    mut network_mapping: ResMut<NetworkMapping>,
    player_assets: Res<PlayerAssets>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    mut player_data: Query<&mut Player>,
    player_transforms: Query<&Transform, With<Player>>,
    mut toggles: EventWriter<ToggleReadyEvent>,
    mut next_screen: ResMut<NextState<Screen>>,
    world_objects: Query<Entity, With<ClientWorldObject>>,
    mut match_time: ResMut<MatchTimeRemaining>,
) {
    let client_id = client_id.0;
    while let Some(message) = client.receive_message(ServerChannel::ServerMessages) {
        let server_message = bincode::deserialize(&message).unwrap();
        match server_message {
            ServerMessages::PlayerCreate {
                id,
                translation,
                entity,
                is_ready,
                color,
            } => {
                println!("Player {} connected.", id);
                let layout = TextureAtlasLayout::from_grid(
                    UVec2::splat(32),
                    6,
                    2,
                    Some(UVec2::splat(1)),
                    None,
                );
                let texture_atlas_layout = texture_atlas_layouts.add(layout);
                let player_animation = PlayerAnimation::new();

                let mut client_entity = commands.spawn((
                    Name::new("Player"),
                    Player {
                        id,
                        score: 0,
                        is_ready,
                        color,
                    },
                    Sprite {
                        image: player_assets.ducky.clone(),
                        texture_atlas: Some(TextureAtlas {
                            layout: texture_atlas_layout.clone(),
                            index: player_animation.get_atlas_index(),
                        }),
                        color: player_color_tint(color),
                        ..default()
                    },
                    Collider {
                        size: Vec2::new(14., 24.),
                        collides_with_player: true,
                        collides_with_projectile: true,
                    },
                    FacingDirection(Vec2::new(0.0, 1.0)),
                    Transform::from_translation(Vec3::from_array(translation)),
                    player_animation,
                    StateScoped(Screen::Gameplay),
                ));

                if client_id == id {
                    client_entity.insert(ControlledPlayer);
                }

                let player_info = PlayerInfo {
                    server_entity: entity,
                    client_entity: client_entity.id(),
                };
                lobby.players.insert(id, player_info);
                network_mapping.0.insert(entity, client_entity.id());
            }
            ServerMessages::PlayerRemove { id } => {
                println!("Player {} disconnected.", id);
                if let Some(PlayerInfo {
                    server_entity,
                    client_entity,
                }) = lobby.players.remove(&id)
                {
                    commands.entity(client_entity).despawn();
                    network_mapping.0.remove(&server_entity);
                }
            }
            ServerMessages::SpawnGameObject { id, translation } => {
                println!("Object {} spawned at {:?}.", id, translation);
                let obj_collider_sizes = [
                    Vec2::new(0., 0.),
                    Vec2::new(90., 76.),
                    Vec2::new(26., 30.),
                    Vec2::new(64., 48.),
                    Vec2::new(94., 48.),
                    Vec2::new(32., 80.),
                    Vec2::new(32., 114.),
                ];
                commands.spawn((
                    Name::new("Dirt"),
                    ClientWorldObject,
                    Sprite {
                        image: match id {
                            0 => player_assets.dirt_patch.clone(),
                            1 => player_assets.pond.clone(),
                            2 => player_assets.trees.clone(),
                            3 => player_assets.wall_h_small.clone(),
                            4 => player_assets.wall_h_large.clone(),
                            5 => player_assets.wall_v_small.clone(),
                            6 => player_assets.wall_v_large.clone(),
                            _ => unreachable!(),
                        },
                        ..default()
                    },
                    Collider {
                        size: obj_collider_sizes[id as usize] * 1.5,
                        collides_with_player: id != 0,
                        collides_with_projectile: id >= 2,
                    },
                    Transform::from_translation(Vec3::from_array(translation))
                        .with_scale(Vec3::new(1.5, 1.5, 1.)),
                    StateScoped(Screen::Gameplay),
                ));
            }
            ServerMessages::SpawnProjectile {
                entity,
                translation,
                angle,
            } => {
                let projectile_entity = commands.spawn((
                    Sprite {
                        image: player_assets.bullet.clone(),
                        custom_size: Some(Vec2::new(12., 18.)),
                        ..default()
                    },
                    Collider {
                        size: Vec2::new(12., 18.),
                        collides_with_player: true,
                        collides_with_projectile: true,
                    },
                    Transform::from_translation(translation.into())
                        .with_rotation(Quat::from_rotation_z(angle)),
                    StateScoped(Screen::Gameplay),
                ));

                network_mapping.0.insert(entity, projectile_entity.id());
            }
            ServerMessages::SpawnCoin {
                entity,
                translation,
            } => {
                let coin_entity = commands.spawn((
                    Sprite {
                        image: player_assets.coin.clone(),
                        ..default()
                    },
                    Collider {
                        size: COIN_COLLIDER_SIZE,
                        collides_with_player: true,
                        collides_with_projectile: false,
                    },
                    Transform::from_translation(translation.into())
                        .with_scale(Vec3::new(COIN_SCALE, COIN_SCALE, 1.)),
                    StateScoped(Screen::Gameplay),
                ));

                network_mapping.0.insert(entity, coin_entity.id());
            }
            ServerMessages::DespawnEntity { entity } => {
                if let Some(entity) = network_mapping.0.remove(&entity) {
                    commands.entity(entity).despawn();
                }
            }
            ServerMessages::SetPlayerReady { entity, is_ready } => {
                if let Some(client_entity) = network_mapping.0.get(&entity) {
                    if let Ok(mut player) = player_data.get_mut(*client_entity) {
                        player.is_ready = is_ready;
                        bevy::log::info!("Player {:?} is ready: {:?}", client_entity, is_ready);
                        toggles.send(ToggleReadyEvent {
                            player: *client_entity,
                            is_ready,
                        });
                    }
                }
            }
            ServerMessages::StartGame => {
                bevy::log::info!("Starting game!");
                match_time.0 = 60;
                commands.insert_resource(LocalDashCooldown::ready());
                commands.insert_resource(LocalShootCooldown::ready());
                next_screen.set(Screen::Gameplay);
            }
            ServerMessages::ReturnToTitle => {
                bevy::log::info!("Server reset — returning to title");
                let world_ents: Vec<Entity> = world_objects.iter().collect();
                clear_client_session(
                    &mut commands,
                    &mut lobby,
                    &mut network_mapping,
                    &world_ents,
                );
                // Leave gameplay/lobby immediately; teardown drops the socket next frame.
                next_screen.set(Screen::Title);
                commands.insert_resource(PendingSessionTeardown);
            }
            ServerMessages::MatchTimer { remaining_secs } => {
                match_time.0 = remaining_secs;
            }
            ServerMessages::MatchEnded { rankings } => {
                bevy::log::info!("Match ended — showing leaderboard");
                let world_ents: Vec<Entity> = world_objects.iter().collect();
                clear_client_session(
                    &mut commands,
                    &mut lobby,
                    &mut network_mapping,
                    &world_ents,
                );
                commands.insert_resource(MatchResults { rankings });
                next_screen.set(Screen::Results);
            }
            ServerMessages::PlayerHit { entity } => {
                let Some(&client_entity) = network_mapping.0.get(&entity) else {
                    continue;
                };
                let Ok(transform) = player_transforms.get(client_entity) else {
                    continue;
                };
                let spawn_at = transform.translation + Vec3::new(0.0, 36.0, 30.0);
                commands.spawn((
                    Name::new("Hit Flash"),
                    Text2d::new("HIT!"),
                    TextFont::from_font_size(32.0),
                    TextColor(Color::srgba(1.0, 0.12, 0.12, 1.0)),
                    TextLayout::new_with_justify(JustifyText::Center),
                    Transform::from_translation(spawn_at),
                    HitFlashText {
                        timer: Timer::from_seconds(3.0, TimerMode::Once),
                    },
                    StateScoped(Screen::Gameplay),
                ));
            }
        }
    }

    while let Some(message) = client.receive_message(ServerChannel::NetworkedEntities) {
        let networked_entities: NetworkedEntities = bincode::deserialize(&message).unwrap();
        for i in 0..networked_entities.entities.len() {
            let Some(&entity) = network_mapping.0.get(&networked_entities.entities[i]) else {
                continue;
            };
            let translation = networked_entities.translations[i].into();
            let maybe_direction = networked_entities.facing_directions[i].map(Vec2::from_array);
            let mut transform = Transform {
                translation,
                ..Default::default()
            };
            if let Some(score) = networked_entities.score[i] {
                if let Ok(mut player) = player_data.get_mut(entity) {
                    player.score = score;
                    transform.scale = Vec3::new(
                        1.0 + calculate_score_growth(score),
                        1.0 + calculate_score_growth(score),
                        1.0,
                    );
                }
            }
            let Some(mut entity_commands) = commands.get_entity(entity) else {
                continue;
            };
            if let Some(direction) = maybe_direction {
                entity_commands.insert(FacingDirection(direction));
            }
            entity_commands.insert(transform);
        }
    }
}

fn update_score_text(
    mut score_text_query: Query<&mut Text, With<ScoreText>>,
    player_data: Query<&Player, With<ControlledPlayer>>,
) {
    for mut text in &mut score_text_query {
        let Ok(player) = player_data.get_single() else {
            return;
        };

        text.0 = format!("Coins: {}", player.score);
    }
}

fn attach_dash_cooldown_bar(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    players: Query<Entity, (With<ControlledPlayer>, Without<DashCooldownBarRoot>)>,
) {
    for player in &players {
        let bg = commands
            .spawn((
                Name::new("Dash CD BG"),
                Mesh2d(meshes.add(Rectangle::new(36.0, 5.0))),
                MeshMaterial2d(materials.add(Color::srgba(0.1, 0.1, 0.1, 0.75))),
                Transform::from_xyz(0.0, 28.0, 2.0),
            ))
            .id();
        let fill = commands
            .spawn((
                Name::new("Dash CD Fill"),
                DashCooldownBarFill,
                Mesh2d(meshes.add(Rectangle::new(36.0, 5.0))),
                MeshMaterial2d(materials.add(Color::srgb(0.25, 0.85, 0.45))),
                Transform::from_xyz(0.0, 28.0, 2.1),
            ))
            .id();
        commands
            .entity(player)
            .insert(DashCooldownBarRoot)
            .add_child(bg)
            .add_child(fill);
    }
}

fn update_dash_cooldown_bar(
    dash_cd: Res<LocalDashCooldown>,
    mut fills: Query<&mut Transform, With<DashCooldownBarFill>>,
) {
    // Ready = full bar; on cooldown the fill grows back from empty → full.
    let ready_frac = if dash_cd.0.finished() {
        1.0
    } else {
        dash_cd.0.fraction()
    };
    for mut transform in &mut fills {
        transform.scale.x = ready_frac.clamp(0.05, 1.0);
        // Keep the bar left-anchored as it shrinks/grows.
        transform.translation.x = -18.0 * (1.0 - transform.scale.x);
    }
}

fn update_hit_flash_text(
    time: Res<Time>,
    mut commands: Commands,
    mut flashes: Query<(Entity, &mut HitFlashText, &mut TextColor, &mut Transform)>,
) {
    for (entity, mut flash, mut color, mut transform) in &mut flashes {
        flash.timer.tick(time.delta());
        let alpha = 1.0 - flash.timer.fraction();
        color.0 = Color::srgba(1.0, 0.12, 0.12, alpha);
        transform.translation.y += 28.0 * time.delta_secs();
        if flash.timer.finished() {
            commands.entity(entity).despawn_recursive();
        }
    }
}

fn spawn_match_timer_ui(
    mut commands: Commands,
    existing: Query<Entity, With<MatchTimerText>>,
    remaining: Res<MatchTimeRemaining>,
) {
    if !existing.is_empty() {
        return;
    }
    commands
        .spawn((
            Name::new("Match Timer Root"),
            StateScoped(Screen::Gameplay),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(12.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::FlexStart,
                ..default()
            },
        ))
        .with_children(|parent| {
            parent.spawn((
                Name::new("Match Timer"),
                MatchTimerText,
                Text::new(format!("{}", remaining.0)),
                TextFont::from_font_size(48.0),
                TextColor(Color::srgb(0.95, 0.15, 0.12)),
                TextLayout::new_with_justify(JustifyText::Center),
            ));
        });
}

fn update_match_timer_text(
    remaining: Res<MatchTimeRemaining>,
    mut timer_text: Query<&mut Text, With<MatchTimerText>>,
) {
    for mut text in &mut timer_text {
        let next = format!("{}", remaining.0);
        if text.0 != next {
            text.0 = next;
        }
    }
}

fn return_to_title_on_disconnect(
    mut commands: Commands,
    mut lobby: ResMut<ClientLobby>,
    mut network_mapping: ResMut<NetworkMapping>,
    world_objects: Query<Entity, With<ClientWorldObject>>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    bevy::log::info!("Lost server connection — returning to title");
    let world_ents: Vec<Entity> = world_objects.iter().collect();
    clear_client_session(
        &mut commands,
        &mut lobby,
        &mut network_mapping,
        &world_ents,
    );
    next_screen.set(Screen::Title);
    commands.insert_resource(PendingSessionTeardown);
}

fn clear_client_session(
    commands: &mut Commands,
    lobby: &mut ClientLobby,
    network_mapping: &mut NetworkMapping,
    world_objects: &[Entity],
) {
    // Despawn every networked entity (players, bullets, coins, etc.).
    let mapped: Vec<Entity> = network_mapping.0.values().copied().collect();
    for entity in mapped {
        commands.entity(entity).despawn_recursive();
    }
    lobby.players.clear();
    network_mapping.0.clear();
    for entity in world_objects {
        commands.entity(*entity).despawn_recursive();
    }
}

fn apply_pending_session_teardown(
    mut commands: Commands,
    pending: Option<Res<PendingSessionTeardown>>,
    transport: Option<ResMut<NetcodeClientTransport>>,
) {
    if pending.is_none() {
        return;
    }
    commands.remove_resource::<PendingSessionTeardown>();
    // Disconnect before dropping resources so the server sees a clean leave.
    if let Some(mut transport) = transport {
        transport.disconnect();
    }
    commands.remove_resource::<RenetClient>();
    commands.remove_resource::<NetcodeClientTransport>();
    commands.remove_resource::<CurrentClientId>();
}
