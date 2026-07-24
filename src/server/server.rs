use crate::{
    demo::{
        animation::FacingDirection,
        client::PLAYER_BASE_COLLIDER_SIZE,
        lib::{
            connection_config, ClientChannel, LeaderboardEntry, NetworkedEntities, Player,
            PlayerCommand, PlayerInput, PublicServerStatus, ServerChannel, ServerMessages,
            ServerPhase, Velocity, MAX_PLAYERS, PLAYER_COLOR_COUNT, PROTOCOL_ID,
        },
        movement::{apply_map_bounds, apply_movement, MovementController},
        physics::{check_collision, Collider},
        player::{Coin, PlayerAssets},
    },
    screens::{
        gameplay::{handle_score_event, spawn_coin, ScoreEvent},
        Screen,
    },
    AppSet,
};
use bevy::{
    app::ScheduleRunnerPlugin,
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    input::InputPlugin,
    prelude::*,
    state::app::StatesPlugin,
};
use bevy_renet2::netcode::NetcodeServerPlugin;
use bevy_renet2::prelude::{
    ClientId, ConnectionConfig, RenetServer, RenetServerPlugin, ServerEvent,
};
use enfync::AdoptOrDefault;
use enfync::Handle;
use std::{
    collections::HashMap,
    f32::consts::PI,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

use rand::Rng;
use renet2_netcode::{
    NativeSocket, ServerAuthentication, ServerCertHash, ServerSetupConfig, WebServerDestination,
};
use renet2_setup::{
    setup_combo_renet2_server_in_bevy, ClientCounts, ConnectMetas, ConnectionType,
    GameServerSetupConfig,
};
use serde::Deserialize;

#[derive(Component)]
pub struct ServerGameObject(pub u64);

#[derive(Debug, Default, Resource)]
pub struct ServerLobby {
    pub players: HashMap<ClientId, Entity>,
}
#[derive(Debug, Default, Resource)]
pub struct CoinSpawner {
    pub timer: Timer,
}

const PLAYER_MOVE_SPEED: f32 = 300.0;
const PROJECTILE_MOVE_SPEED: f32 = 500.0;
const MATCH_DURATION_SECS: f32 = 60.0;
const DASH_DURATION_SECS: f32 = 0.16;
const DASH_COOLDOWN_SECS: f32 = 2.5;
/// Strong short burst — total distance ≈ base_speed * mult * duration.
const DASH_SPEED_MULT: f32 = 4.0;
/// Delay so ReturnToTitle / MatchEnded reach clients before sockets close.
const DISCONNECT_DELAY_SECS: f32 = 0.75;
const SPAWN_POSITIONS: [Vec2; 8] = [
    Vec2::new(-250., 0.),
    Vec2::new(250., 0.),
    Vec2::new(0., 250.),
    Vec2::new(0., -250.),
    Vec2::new(176., 176.),
    Vec2::new(-176., 176.),
    Vec2::new(-176., -176.),
    Vec2::new(176., -176.),
];

#[derive(Debug, Component)]
struct Bot {
    auto_cast: Timer,
}

#[derive(Debug, Resource)]
struct BotId(u64);

#[derive(Debug, Resource)]
struct MatchClock {
    remaining: f32,
    last_broadcast_secs: u16,
}

#[derive(Debug, Resource)]
struct PendingDisconnectAll {
    timer: Timer,
}

/// Shared with the HTTP task so the main menu can poll lobby/match status.
#[derive(Resource, Clone)]
struct SharedServerStatus(Arc<Mutex<PublicServerStatus>>);

impl SharedServerStatus {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(PublicServerStatus::default())))
    }
}

#[derive(Component, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct Projectile {
    pub speed: f32,
    pub direction: Vec2,
    pub owner: Entity,
}

#[derive(Component, Debug)]
struct DashState {
    cooldown: Timer,
    active: Timer,
    direction: Vec2,
}

impl Default for DashState {
    fn default() -> Self {
        let mut cooldown = Timer::from_seconds(DASH_COOLDOWN_SECS, TimerMode::Once);
        cooldown.set_elapsed(cooldown.duration());
        let mut active = Timer::from_seconds(DASH_DURATION_SECS, TimerMode::Once);
        active.set_elapsed(active.duration());
        Self {
            cooldown,
            active,
            direction: Vec2::Y,
        }
    }
}

// // #[cfg(feature = "netcode")]
// fn setup_udp_server(app: &mut App) {
//     println!("Setting up UDP server");
//     use bevy_renet2::netcode::{
//         NetcodeServerPlugin, NetcodeServerTransport, ServerAuthentication, ServerConfig,
//     };
//     use std::{net::UdpSocket, time::SystemTime};

//     app.add_plugins(NetcodeServerPlugin);

//     let server = RenetServer::new(connection_config());

//     let public_addr = "0.0.0.0:8080".parse().unwrap();
//     let socket = UdpSocket::bind(public_addr).unwrap();
//     let current_time: std::time::Duration = SystemTime::now()
//         .duration_since(SystemTime::UNIX_EPOCH)
//         .unwrap();
//     let server_config = ServerSetupConfig {
//         current_time,
//         max_clients: 64,
//         protocol_id: PROTOCOL_ID,
//         socket_addresses: vec![vec![public_addr]],
//         authentication: ServerAuthentication::Unsecure,
//     };

//     let transport =
//         NetcodeServerTransport::new(server_config, NativeSocket::new(socket).unwrap()).unwrap();
//     app.insert_resource(server);
//     app.insert_resource(transport);
//     println!(
//         "UDP Server successfully initialized on address: {}",
//         public_addr
//     );
// }

struct ClientConnectionInfo {
    native_addr: String,
    wt_dest: WebServerDestination,
    ws_url: url::Url,
    cert_hash: ServerCertHash,
}

fn setup_server(world: &mut World) {
    println!("Setting up server");
    let bind_ip = std::env::var("CHEXY_SERVER_BIND_IP")
        .ok()
        .and_then(|value| value.parse::<IpAddr>().ok())
        .unwrap_or(IpAddr::from([0, 0, 0, 0]));
    let public_ip = std::env::var("CHEXY_SERVER_PUBLIC_IP")
        .ok()
        .and_then(|value| value.parse::<IpAddr>().ok())
        .unwrap_or_else(|| {
            if bind_ip.is_unspecified() {
                IpAddr::V4(Ipv4Addr::LOCALHOST)
            } else {
                bind_ip
            }
        });
    let http_port = std::env::var("CHEXY_SERVER_HTTP_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8080);
    let native_port = std::env::var("CHEXY_SERVER_NATIVE_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8081);
    let wasm_wt_port = std::env::var("CHEXY_SERVER_WT_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8082);
    let wasm_ws_port = std::env::var("CHEXY_SERVER_WS_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8083);
    let native_port_proxy = std::env::var("CHEXY_SERVER_NATIVE_PORT_PROXY")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(0);
    let wasm_wt_port_proxy = std::env::var("CHEXY_SERVER_WT_PORT_PROXY")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(0);
    let wasm_ws_port_proxy = std::env::var("CHEXY_SERVER_WS_PORT_PROXY")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(0);
    let ws_domain = std::env::var("CHEXY_SERVER_WS_DOMAIN")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let has_wss_proxy = std::env::var("CHEXY_SERVER_HAS_WSS_PROXY")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false);

    println!(
        "Server binding to {bind_ip} (public {public_ip}) http:{http_port} native:{native_port} wt:{wasm_wt_port} ws:{wasm_ws_port}"
    );

    let config: GameServerSetupConfig = GameServerSetupConfig {
        protocol_id: PROTOCOL_ID,
        expire_secs: 100000000u64,
        timeout_secs: 100000000i32,
        server_ip: bind_ip,
        native_port,
        wasm_wt_port,
        wasm_ws_port,
        proxy_ip: Some(public_ip),
        ws_domain,
        wss_certs: None,
        native_port_proxy,
        wasm_wt_port_proxy,
        wasm_ws_port_proxy,
        has_wss_proxy,
    };
    let client_counts: ClientCounts = ClientCounts {
        memory_clients: vec![],
        native_count: 16,
        wasm_wt_count: 16,
        wasm_ws_count: 16,
    };

    let connect_metas =
        setup_combo_renet2_server_in_bevy(world, config, client_counts, connection_config())
            .expect("Server failed to start");

    let status = world.resource::<SharedServerStatus>().0.clone();
    let runtime = enfync::builtin::native::TokioHandle::adopt_or_default();
    runtime.spawn(async move {
        run_http_server(
            SocketAddr::new(bind_ip, http_port),
            connect_metas,
            status,
        )
        .await
    });
    println!("Server setup complete");
}

async fn run_http_server(
    http_addr: SocketAddr,
    connect_metas: ConnectMetas,
    status: Arc<Mutex<PublicServerStatus>>,
) {
    use warp::Filter;

    let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec!["GET", "POST", "OPTIONS"])
        .allow_headers(vec!["Content-Type"]);

    use std::convert::Infallible;

    let auth = warp::post()
        .and(warp::path("auth"))
        .and(warp::path::param::<u64>())
        .and(warp::query::<TokenRequest>().or_else(|_| async move {
            Ok::<(TokenRequest,), Infallible>((TokenRequest::default(),))
        }))
        .map(move |id, request: TokenRequest| token_handler(connect_metas.clone(), id, request));

    let status_state = status.clone();
    let status_route = warp::get()
        .and(warp::path("status"))
        .and(warp::path::end())
        .map(move || {
            let snapshot = status_state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            warp::reply::json(&snapshot)
        });

    let routes = auth.or(status_route).with(cors);
    warp::serve(routes).run(http_addr).await;
}

#[derive(Debug, Default, Deserialize)]
struct TokenRequest {
    transport: Option<String>,
}

fn parse_connection_type(value: Option<String>) -> ConnectionType {
    fn parse(value: &str) -> Option<ConnectionType> {
        match value.to_ascii_lowercase().as_str() {
            "memory" => Some(ConnectionType::Memory),
            "native" => Some(ConnectionType::Native),
            "wasm_wt" | "wasm-wt" => Some(ConnectionType::WasmWt),
            "wasm_ws" | "wasm-ws" => Some(ConnectionType::WasmWs),
            _ => None,
        }
    }

    value
        .as_deref()
        .and_then(parse)
        .unwrap_or(ConnectionType::WasmWs)
}

fn token_handler(
    connect_metas: ConnectMetas,
    client_id: u64,
    request: TokenRequest,
) -> impl warp::Reply {
    println!(
        "GOT TOKEN REQUEST {client_id:?} transport={:?}",
        request.transport
    );
    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let connection_type = parse_connection_type(request.transport);
    let token = connect_metas
        .new_connect_token(current_time, client_id, connection_type)
        .unwrap_or_else(|err| panic!("failed to create connect token: {err}"));
    warp::reply::json(&token)
}

// #[cfg(feature = "server")]
// fn setup_wasm_server(app: &mut App) {
//     println!("Setting up WASM server");
//     use renet2_netcode::{
//         BoxedSocket, NativeSocket, NetcodeServerTransport, ServerCertHash, ServerSetupConfig,
//         ServerSocket, WebServerDestination, WebSocketServer, WebSocketServerConfig,
//         WebTransportServer, WebTransportServerConfig,
//     };
//     let runtime = tokio::runtime::Runtime::new().unwrap();

//     let http_addr: SocketAddr = "0.0.0.0:8080".parse().unwrap();
//     let max_clients = 10;

//     // Native socket
//     let wildcard_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
//     let native_socket = NativeSocket::new(UdpSocket::bind(wildcard_addr).unwrap()).unwrap();

//     // WebTransport socket
//     let (wt_socket, cert_hash) = {
//         let (config, cert_hash) =
//             WebTransportServerConfig::new_selfsigned(wildcard_addr, max_clients).unwrap();
//         (
//             WebTransportServer::new(config, runtime.handle().clone()).unwrap(),
//             cert_hash,
//         )
//     };

//     // WebSocket socket
//     let ws_socket = {
//         let config = WebSocketServerConfig::new(wildcard_addr, max_clients);
//         WebSocketServer::new(config, runtime.handle().clone()).unwrap()
//     };

//     // Save connection info
//     let client_connection_info = ClientConnectionInfo {
//         native_addr: native_socket.addr().unwrap().to_string(),
//         wt_dest: wt_socket.addr().unwrap().into(),
//         ws_url: ws_socket.url(),
//         cert_hash,
//     };

//     // Setup netcode server transport
//     let current_time = SystemTime::now()
//         .duration_since(SystemTime::UNIX_EPOCH)
//         .unwrap();
//     let server_config = ServerSetupConfig {
//         current_time,
//         max_clients,
//         protocol_id: 0,
//         socket_addresses: vec![
//             vec![native_socket.addr().unwrap()],
//             vec![wt_socket.addr().unwrap()],
//             vec![ws_socket.addr().unwrap()],
//         ],
//         authentication: ServerAuthentication::Unsecure,
//     };
//     let transport = NetcodeServerTransport::new_with_sockets(
//         server_config,
//         Vec::from([
//             BoxedSocket::new(native_socket),
//             BoxedSocket::new(wt_socket),
//             BoxedSocket::new(ws_socket),
//         ]),
//     )
//     .unwrap();
//     debug!("transport created");

//     // Run HTTP server for clients to get connection info.
//     runtime.spawn(async move { run_http_server(http_addr, client_connection_info).await });

//     let server = RenetServer::new(connection_config());
//     app.insert_resource(server);
//     app.insert_resource(transport);
//     println!("DONE Setting up WASM server");
// }

pub struct ServerPlugin;

impl Plugin for ServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(
            Duration::from_secs_f64(1.0 / 60.0),
        )));
        app.add_plugins((
            StatesPlugin,
            InputPlugin,
            RenetServerPlugin,
            NetcodeServerPlugin,
            FrameTimeDiagnosticsPlugin,
            LogDiagnosticsPlugin::default(),
        ));
        // app.add_plugins(EguiPlugin);

        app.insert_resource(ServerLobby::default());
        app.insert_resource(BotId(0));
        app.insert_resource(CoinSpawner {
            timer: Timer::from_seconds(1.2, TimerMode::Repeating),
        });
        app.insert_resource(SharedServerStatus::new());

        app.init_state::<Screen>();
        app.add_systems(Update, handle_score_event);

        app.add_event::<ScoreEvent>();

        app.add_systems(Startup, setup_server);

        app.add_systems(
            Update,
            (
                // Movement before despawn/reset so we never queue inserts on removed players.
                move_players_system.run_if(in_state(Screen::Gameplay)),
                server_update_system,
                tick_match_clock
                    .run_if(in_state(Screen::Gameplay))
                    .run_if(resource_exists::<MatchClock>),
                tick_pending_disconnect_all,
                publish_server_status,
                server_network_sync,
                spawn_bot.run_if(in_state(Screen::Gameplay)),
                bot_autocast.run_if(in_state(Screen::Gameplay)),
            )
                .chain(),
        );
        app.add_systems(
            Update,
            (apply_movement, apply_map_bounds)
                .chain()
                .in_set(AppSet::Update)
                .run_if(in_state(Screen::Gameplay)),
        );

        app.add_systems(
            FixedUpdate,
            (
                move_projectiles,
                spawn_coins.run_if(in_state(Screen::Gameplay)),
            ),
        );
        app.add_systems(Startup, generate_world);

        app.add_systems(
            PostUpdate,
            (projectile_on_removal_system, coin_on_removal_system),
        );

        // app.add_systems(Startup, setup_simple_camera);
    }
}

#[allow(clippy::too_many_arguments)]
fn server_update_system(
    mut server_events: EventReader<ServerEvent>,
    mut commands: Commands,
    mut lobby: ResMut<ServerLobby>,
    mut server: ResMut<RenetServer>,
    mut players: Query<(
        Entity,
        &mut Player,
        &Transform,
        &MovementController,
        &FacingDirection,
    )>,
    mut dash_states: Query<&mut DashState>,
    game_objects: Query<(Entity, &Transform, &ServerGameObject)>,
    coins_query: Query<Entity, With<Coin>>,
    projectiles_query: Query<Entity, With<Projectile>>,
    mut next_screen: ResMut<NextState<Screen>>,
    screen: Res<State<Screen>>,
) {
    let in_match = *screen.get() == Screen::Gameplay;
    for event in server_events.read() {
        println!("TEST: {:?}", event);
        match event {
            ServerEvent::ClientConnected { client_id } => {
                println!("Player {} connected.", client_id);

                // Initialize other players for this new client
                for (entity, player, transform, _, _) in players.iter() {
                    let translation: [f32; 3] = transform.translation.into();
                    let message = bincode::serialize(&ServerMessages::PlayerCreate {
                        id: player.id,
                        entity,
                        translation,
                        is_ready: player.is_ready,
                        color: player.color,
                    })
                    .unwrap();
                    server.send_message(*client_id, ServerChannel::ServerMessages, message);
                }

                // Initialize game objects for this player
                for (_entity, transform, id) in game_objects.iter() {
                    let translation: [f32; 3] = transform.translation.into();
                    let message = bincode::serialize(&ServerMessages::SpawnGameObject {
                        id: id.0,
                        translation,
                    })
                    .unwrap();
                    server.send_message(*client_id, ServerChannel::ServerMessages, message);
                }
                // Spawn new player
                let color = (lobby.players.len() % PLAYER_COLOR_COUNT) as u8;
                let transform = Transform::from_translation(
                    SPAWN_POSITIONS[lobby.players.len() % SPAWN_POSITIONS.len()].extend(8.),
                );
                let player_entity = commands
                    .spawn((
                        transform,
                        MovementController {
                            max_speed: PLAYER_MOVE_SPEED,
                            ..default()
                        },
                    ))
                    .insert(Collider {
                        size: PLAYER_BASE_COLLIDER_SIZE,
                        collides_with_player: true,
                        collides_with_projectile: true,
                    })
                    .insert(PlayerInput::default())
                    .insert(Velocity::default())
                    .insert(FacingDirection(Vec2::Y))
                    .insert(DashState::default())
                    .insert(Player {
                        id: *client_id,
                        score: 0,
                        is_ready: false,
                        color,
                    })
                    .id();
                println!("PLAYER ENTITY: {player_entity:?}");

                lobby.players.insert(*client_id, player_entity);

                let translation: [f32; 3] = transform.translation.into();
                let message = bincode::serialize(&ServerMessages::PlayerCreate {
                    id: *client_id,
                    entity: player_entity,
                    translation,
                    is_ready: false,
                    color,
                })
                .unwrap();
                server.broadcast_message(ServerChannel::ServerMessages, message);
            }
            ServerEvent::ClientDisconnected { client_id, reason } => {
                println!("Player {} disconnected: {}", client_id, reason);
                if let Some(player_entity) = lobby.players.remove(client_id) {
                    commands.entity(player_entity).despawn();
                }

                let message =
                    bincode::serialize(&ServerMessages::PlayerRemove { id: *client_id }).unwrap();
                server.broadcast_message(ServerChannel::ServerMessages, message);
            }
        }
    }

    for client_id in server.clients_id() {
        while let Some(message) = server.receive_message(client_id, ClientChannel::Command) {
            let command: PlayerCommand = bincode::deserialize(&message).unwrap();
            match command {
                PlayerCommand::BasicAttack { aim } => {
                    if !in_match {
                        continue;
                    }
                    println!("Received basic attack from client {}", client_id);

                    if let Some(player_entity) = lobby.players.get(&client_id) {
                        if let Ok((_, _, player_transform, _, facing)) =
                            players.get(*player_entity)
                        {
                            let aim = Vec2::from_array(aim);
                            let mut player_dir =
                                (aim - player_transform.translation.xy()).normalize_or_zero();
                            if player_dir == Vec2::ZERO {
                                player_dir = facing.0.normalize_or_zero();
                            }
                            if player_dir == Vec2::ZERO {
                                continue;
                            }
                            let angle =
                                player_dir.y.atan2(player_dir.x) - std::f32::consts::PI / 2.0;

                            let offset_distance = 20.0; // How far in front of the player to spawn the projectile
                            let offset = player_dir * offset_distance;
                            let spawn_position = player_transform.translation.xy() + offset;

                            let final_translation = player_transform
                                .with_translation(spawn_position.extend(10.))
                                .translation;

                            let projectile_entity = commands
                                .spawn((Transform::from_translation(final_translation)
                                    .with_rotation(Quat::from_rotation_z(angle)),))
                                .insert(Collider {
                                    size: Vec2::new(12., 18.),
                                    collides_with_player: true,
                                    collides_with_projectile: true,
                                })
                                .insert(FacingDirection(player_dir))
                                .insert(Projectile {
                                    speed: PROJECTILE_MOVE_SPEED,
                                    direction: player_dir,
                                    owner: player_entity.clone(),
                                })
                                .id();
                            let message = ServerMessages::SpawnProjectile {
                                entity: projectile_entity,
                                translation: final_translation.into(),
                                angle,
                            };
                            let message = bincode::serialize(&message).unwrap();
                            server.broadcast_message(ServerChannel::ServerMessages, message);
                        }
                    }
                }
                PlayerCommand::Dash => {
                    if !in_match {
                        continue;
                    }
                    if let Some(player_entity) = lobby.players.get(&client_id) {
                        let Ok((_, _, _, movement, facing)) = players.get(*player_entity) else {
                            continue;
                        };
                        let Ok(mut dash) = dash_states.get_mut(*player_entity) else {
                            continue;
                        };
                        if !dash.cooldown.finished() {
                            continue;
                        }
                        let dir = if movement.intent != Vec2::ZERO {
                            movement.intent
                        } else {
                            facing.0.normalize_or_zero()
                        };
                        if dir == Vec2::ZERO {
                            continue;
                        }
                        dash.direction = dir;
                        dash.active.reset();
                        dash.cooldown.reset();
                        println!("Player {client_id} dashed toward {dir:?}");
                    }
                }
                PlayerCommand::DebugSpawnBot => {
                    if !in_match {
                        continue;
                    }
                    println!("Received debug spawn bot command from client {}", client_id);
                    if let Some(player_entity) = lobby.players.get(&client_id) {
                        if let Ok((_, _, _player_transform, _, _)) = players.get(*player_entity) {
                            let color = (lobby.players.len() % PLAYER_COLOR_COUNT) as u8;
                            let transform = Transform::from_translation(
                                SPAWN_POSITIONS[lobby.players.len() % SPAWN_POSITIONS.len()]
                                    .extend(8.),
                            );
                            let player_entity = commands
                                .spawn((
                                    transform,
                                    MovementController {
                                        max_speed: PLAYER_MOVE_SPEED,
                                        ..default()
                                    },
                                    PlayerInput::default(),
                                    FacingDirection(Vec2::Y),
                                    DashState::default(),
                                    Player {
                                        id: client_id,
                                        score: 0,
                                        is_ready: true,
                                        color,
                                    },
                                ))
                                .id();
                            let current_time = SystemTime::now()
                                .duration_since(SystemTime::UNIX_EPOCH)
                                .unwrap()
                                .as_secs();
                            lobby.players.insert(current_time, player_entity);

                            let translation: [f32; 3] = transform.translation.into();
                            let message = bincode::serialize(&ServerMessages::PlayerCreate {
                                id: client_id,
                                entity: player_entity,
                                translation,
                                is_ready: true,
                                color,
                            })
                            .unwrap();
                            server.broadcast_message(ServerChannel::ServerMessages, message);
                        }
                    }
                }
                PlayerCommand::ToggleReady => {
                    println!("Received toggle ready command from client {}", client_id);
                    if let Some(player_entity) = lobby.players.get_mut(&client_id) {
                        println!("     -> Got player Entity: {player_entity:?}");
                        println!("     -> count: {:?}", players.is_empty());
                        if let Ok((_, mut player, _, _, _)) = players.get_mut(*player_entity) {
                            println!("          -> Got player Details");
                            player.is_ready = !player.is_ready;
                            println!("Player {} is now {:?}", client_id, player.is_ready);
                            let message = bincode::serialize(&ServerMessages::SetPlayerReady {
                                entity: *player_entity,
                                is_ready: player.is_ready,
                            })
                            .unwrap();
                            server.broadcast_message(ServerChannel::ServerMessages, message);
                        }
                    }
                    if lobby.players.len() == 1 {
                        continue;
                    }

                    let mut all_players_ready_check = true;
                    for (_, player) in lobby.players.iter() {
                        if let Ok((_, player, _, _, _)) = players.get(*player) {
                            if !player.is_ready {
                                all_players_ready_check = false;
                                break;
                            }
                        }
                    }

                    if all_players_ready_check {
                        println!("All players are ready, starting game!");
                        let message = bincode::serialize(&ServerMessages::StartGame).unwrap();
                        server.broadcast_message(ServerChannel::ServerMessages, message);
                        start_match_clock(&mut commands, &mut server);
                        next_screen.set(Screen::Gameplay);
                    }
                }
                PlayerCommand::ResetServer => {
                    println!("Received reset server command from client {}", client_id);
                    let message = bincode::serialize(&ServerMessages::ReturnToTitle).unwrap();
                    server.broadcast_message(ServerChannel::ServerMessages, message);

                    let player_ents: Vec<Entity> =
                        players.iter().map(|(entity, _, _, _, _)| entity).collect();
                    reset_server_world(
                        &mut commands,
                        &mut lobby,
                        player_ents,
                        &coins_query,
                        &projectiles_query,
                        &game_objects,
                        &mut next_screen,
                    );
                    schedule_disconnect_all(&mut commands);
                }
            }
        }
        while let Some(message) = server.receive_message(client_id, ClientChannel::Input) {
            // Discard movement input while waiting in the match lobby.
            if !in_match {
                continue;
            }
            let input: PlayerInput = bincode::deserialize(&message).unwrap();

            if let Some(player_entity) = lobby.players.get(&client_id) {
                // Avoid Bevy B0003 if this player was just queued for despawn.
                if let Some(mut entity_commands) = commands.get_entity(*player_entity) {
                    entity_commands.insert(input);
                }
            }
        }
    }
}

// fn update_visulizer_system(
//     mut egui_contexts: EguiContexts,
//     mut visualizer: ResMut<RenetServerVisualizer<200>>,
//     server: Res<RenetServer>,
// ) {
//     visualizer.update(&server);
//     visualizer.show_window(egui_contexts.ctx_mut());
// }

#[allow(clippy::type_complexity)]
fn server_network_sync(
    mut server: ResMut<RenetServer>,
    query: Query<
        (
            Entity,
            &Transform,
            Option<&FacingDirection>,
            Option<&Player>,
        ),
        Or<(With<Player>, With<Projectile>)>,
    >,
) {
    let mut networked_entities = NetworkedEntities::default();
    for (entity, transform, maybe_direction, maybe_player) in query.iter() {
        networked_entities.entities.push(entity);
        networked_entities
            .translations
            .push(transform.translation.into());

        networked_entities
            .score
            .push(maybe_player.map(|player| player.score));

        networked_entities.facing_directions.push(
            maybe_direction
                .map(|direction| Some([direction.0.x, direction.0.y]))
                .unwrap_or(None),
        );
    }

    let sync_message = bincode::serialize(&networked_entities).unwrap();
    server.broadcast_message(ServerChannel::NetworkedEntities, sync_message);
}

fn move_players_system(
    time: Res<Time>,
    mut query: Query<(
        &mut MovementController,
        &PlayerInput,
        &mut FacingDirection,
        &mut DashState,
    )>,
) {
    for (mut controller, input, mut facing, mut dash) in query.iter_mut() {
        dash.cooldown.tick(time.delta());

        if !dash.active.finished() {
            dash.active.tick(time.delta());
            controller.intent = dash.direction;
            controller.speed_multiplier = DASH_SPEED_MULT;
            facing.0 = dash.direction;
            continue;
        }

        controller.speed_multiplier = 1.0;
        let x = (input.right as i8 - input.left as i8) as f32;
        let y = (input.up as i8 - input.down as i8) as f32;
        let direction = Vec2::new(x, y).normalize_or_zero();
        controller.intent = direction;
        if direction != Vec2::ZERO {
            facing.0 = direction;
        }
    }
}

fn move_projectiles(
    mut commands: Commands,
    time: Res<Time>,
    mut score_event: EventWriter<ScoreEvent>,
    mut query: Query<(Entity, &Projectile, &mut Transform, &Collider), With<Projectile>>,
    colliders: Query<(Entity, &Transform, &Collider, Option<&Player>), Without<Projectile>>,
    mut server: ResMut<RenetServer>,
) {
    for (e, projectile, mut proj_transform, proj_collider) in &mut query {
        let movement_this_frame =
            projectile.direction.extend(0.0) * projectile.speed * time.delta_secs();
        for (collider_entity, collider_transform, collider, maybe_player) in &colliders {
            //use check_collision

            if collider.collides_with_projectile
                && projectile.owner != collider_entity
                && check_collision(
                    &(proj_transform.translation + movement_this_frame),
                    proj_collider,
                    &collider_transform.translation,
                    collider,
                )
            {
                if let Some(player) = maybe_player {
                    let penalty = i64::min(5, player.score);
                    score_event.send(ScoreEvent {
                        player: collider_entity,
                        delta: -penalty,
                    });
                    let hit_msg =
                        bincode::serialize(&ServerMessages::PlayerHit {
                            entity: collider_entity,
                        })
                        .unwrap();
                    server.broadcast_message(ServerChannel::ServerMessages, hit_msg);
                    for _ in 0..penalty {
                        let mut rng = rand::thread_rng();
                        let player_pos = collider_transform.translation;
                        let x_offset = rng.gen_range(-200.0..200.0); // You can adjust the upper bound here
                        let y_offset = rng.gen_range(-200.0..200.0); // You can adjust the upper bound here
                        let pos = player_pos + Vec3::new(x_offset, y_offset, 3.);
                        spawn_coin(&mut commands, &mut server, pos);
                    }
                }
                // If we're colliding, don't move.
                commands.entity(e).despawn();
                return;
            }
        }
        proj_transform.translation += movement_this_frame
    }
}

pub fn setup_simple_camera(mut commands: Commands) {
    // camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(-20.5, 30.0, 20.5).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn projectile_on_removal_system(
    mut server: ResMut<RenetServer>,
    mut removed_projectiles: RemovedComponents<Projectile>,
) {
    for entity in removed_projectiles.read() {
        let message = ServerMessages::DespawnEntity { entity };
        let message = bincode::serialize(&message).unwrap();

        server.broadcast_message(ServerChannel::ServerMessages, message);
    }
}

fn coin_on_removal_system(
    mut server: ResMut<RenetServer>,
    mut removed_coins: RemovedComponents<Coin>,
) {
    for entity in removed_coins.read() {
        let message = ServerMessages::DespawnEntity { entity };
        let message = bincode::serialize(&message).unwrap();

        server.broadcast_message(ServerChannel::ServerMessages, message);
    }
}

fn spawn_bot(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut lobby: ResMut<ServerLobby>,
    mut server: ResMut<RenetServer>,
    mut bot_id: ResMut<BotId>,
    mut commands: Commands,
) {
    if keyboard_input.just_pressed(KeyCode::KeyB) {
        let client_id: ClientId = bot_id.0;
        bot_id.0 += 1;
        // Spawn new player

        let color = (lobby.players.len() % PLAYER_COLOR_COUNT) as u8;
        let transform = Transform::from_translation(
            SPAWN_POSITIONS[lobby.players.len() % SPAWN_POSITIONS.len()].extend(8.),
        );
        let player_entity = commands
            .spawn((
                transform,
                MovementController {
                    max_speed: PLAYER_MOVE_SPEED,
                    ..default()
                },
                PlayerInput::default(),
                FacingDirection(Vec2::Y),
                DashState::default(),
                Player {
                    id: client_id,
                    score: 0,
                    is_ready: true,
                    color,
                },
                Bot {
                    auto_cast: Timer::from_seconds(1.0, TimerMode::Repeating),
                },
            ))
            .id();

        lobby.players.insert(client_id, player_entity);

        let translation: [f32; 3] = transform.translation.into();
        let message = bincode::serialize(&ServerMessages::PlayerCreate {
            id: client_id,
            entity: player_entity,
            translation,
            is_ready: true,
            color,
        })
        .unwrap();
        server.broadcast_message(ServerChannel::ServerMessages, message);
    }
}

fn bot_autocast(
    time: Res<Time>,
    mut server: ResMut<RenetServer>,
    mut bots: Query<(Entity, &Transform, &mut Bot), With<Player>>,
    mut commands: Commands,
) {
    for (entity, transform, mut bot) in &mut bots {
        bot.auto_cast.tick(time.delta());
        if !bot.auto_cast.just_finished() {
            continue;
        }

        let bot_dir = Vec2::new(fastrand::f32() - 0.5, fastrand::f32() - 0.5).normalize();
        let angle = bot_dir.y.atan2(bot_dir.x) - std::f32::consts::PI / 2.0;

        let offset_distance = 50.0; // How far in front of the player to spawn the projectile
        let offset = bot_dir * offset_distance;
        let spawn_position = transform.translation.xy() + offset;

        let final_translation = transform
            .with_translation(spawn_position.extend(8.))
            .translation;

        let projectile_entity = commands
            .spawn((Transform::from_translation(final_translation)
                .with_rotation(Quat::from_rotation_z(angle)),))
            .insert(Projectile {
                speed: PROJECTILE_MOVE_SPEED,
                direction: bot_dir,
                owner: entity,
            })
            .insert(Collider {
                size: Vec2::new(12., 18.),
                collides_with_player: true,
                collides_with_projectile: true,
            })
            .insert(FacingDirection(bot_dir))
            .id();
        let message = ServerMessages::SpawnProjectile {
            entity: projectile_entity,
            translation: final_translation.into(),
            angle,
        };
        let message = bincode::serialize(&message).unwrap();
        server.broadcast_message(ServerChannel::ServerMessages, message);
    }
}

fn start_match_clock(commands: &mut Commands, server: &mut RenetServer) {
    let secs = MATCH_DURATION_SECS as u16;
    commands.insert_resource(MatchClock {
        remaining: MATCH_DURATION_SECS,
        last_broadcast_secs: secs,
    });
    let message = bincode::serialize(&ServerMessages::MatchTimer {
        remaining_secs: secs,
    })
    .unwrap();
    server.broadcast_message(ServerChannel::ServerMessages, message);
}

fn schedule_disconnect_all(commands: &mut Commands) {
    commands.insert_resource(PendingDisconnectAll {
        timer: Timer::from_seconds(DISCONNECT_DELAY_SECS, TimerMode::Once),
    });
}

fn reset_server_world(
    commands: &mut Commands,
    lobby: &mut ServerLobby,
    player_entities: impl IntoIterator<Item = Entity>,
    coins_query: &Query<Entity, With<Coin>>,
    projectiles_query: &Query<Entity, With<Projectile>>,
    game_objects: &Query<(Entity, &Transform, &ServerGameObject)>,
    next_screen: &mut NextState<Screen>,
) {
    for entity in player_entities {
        commands.entity(entity).despawn_recursive();
    }
    for entity in coins_query.iter().collect::<Vec<_>>() {
        commands.entity(entity).despawn_recursive();
    }
    for entity in projectiles_query.iter().collect::<Vec<_>>() {
        commands.entity(entity).despawn_recursive();
    }
    for entity in game_objects.iter().map(|(entity, _, _)| entity).collect::<Vec<_>>() {
        commands.entity(entity).despawn_recursive();
    }

    lobby.players.clear();
    commands.remove_resource::<MatchClock>();
    next_screen.set(Screen::Lobby);
    spawn_world_objects(commands);
    println!("Server world reset — walls regenerated");
}

fn tick_match_clock(
    mut commands: Commands,
    time: Res<Time>,
    mut clock: ResMut<MatchClock>,
    mut server: ResMut<RenetServer>,
    mut lobby: ResMut<ServerLobby>,
    players: Query<(Entity, &Player)>,
    coins_query: Query<Entity, With<Coin>>,
    projectiles_query: Query<Entity, With<Projectile>>,
    game_objects: Query<(Entity, &Transform, &ServerGameObject)>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    if clock.remaining <= 0.0 {
        // Already ended this match; wait for resource removal.
        return;
    }

    clock.remaining = (clock.remaining - time.delta_secs()).max(0.0);
    // Display whole seconds remaining; show 0 for the final partial second.
    let secs_left = if clock.remaining <= 0.0 {
        0
    } else {
        clock.remaining.ceil() as u16
    };

    if secs_left != clock.last_broadcast_secs {
        clock.last_broadcast_secs = secs_left;
        let message = bincode::serialize(&ServerMessages::MatchTimer {
            remaining_secs: secs_left,
        })
        .unwrap();
        server.broadcast_message(ServerChannel::ServerMessages, message);
        println!("Match timer: {secs_left}s");
    }

    if clock.remaining > 0.0 {
        return;
    }

    let mut rankings: Vec<LeaderboardEntry> = players
        .iter()
        .map(|(_, player)| LeaderboardEntry {
            client_id: player.id,
            score: player.score,
            color: player.color,
        })
        .collect();
    rankings.sort_by(|a, b| b.score.cmp(&a.score));

    println!("Match ended — rankings: {rankings:?}");
    let message = bincode::serialize(&ServerMessages::MatchEnded { rankings }).unwrap();
    server.broadcast_message(ServerChannel::ServerMessages, message);

    // Keep MatchClock at 0 so we don't re-enter end logic before remove applies.
    clock.remaining = 0.0;
    let player_ents: Vec<Entity> = players.iter().map(|(entity, _)| entity).collect();
    reset_server_world(
        &mut commands,
        &mut lobby,
        player_ents,
        &coins_query,
        &projectiles_query,
        &game_objects,
        &mut next_screen,
    );
    schedule_disconnect_all(&mut commands);
}

fn tick_pending_disconnect_all(
    mut commands: Commands,
    time: Res<Time>,
    pending: Option<ResMut<PendingDisconnectAll>>,
    mut server: ResMut<RenetServer>,
) {
    let Some(mut pending) = pending else {
        return;
    };
    if pending.timer.tick(time.delta()).just_finished() {
        println!("Disconnecting all clients after reset flush");
        server.disconnect_all();
        commands.remove_resource::<PendingDisconnectAll>();
    }
}

fn publish_server_status(
    shared: Res<SharedServerStatus>,
    lobby: Res<ServerLobby>,
    screen: Res<State<Screen>>,
    match_clock: Option<Res<MatchClock>>,
) {
    let phase = if *screen.get() == Screen::Gameplay {
        ServerPhase::Match
    } else {
        ServerPhase::Lobby
    };
    let remaining_secs = match_clock.map(|clock| clock.remaining.ceil() as u16);
    let snapshot = PublicServerStatus {
        phase,
        players: lobby.players.len(),
        max_players: MAX_PLAYERS,
        remaining_secs: if phase == ServerPhase::Match {
            remaining_secs
        } else {
            None
        },
    };
    if let Ok(mut guard) = shared.0.lock() {
        *guard = snapshot;
    }
}

fn generate_world(mut commands: Commands) {
    spawn_world_objects(&mut commands);
}

fn spawn_world_objects(commands: &mut Commands) {
    let obj_collider_sizes = [Vec2::new(0., 0.), Vec2::new(110., 80.), Vec2::new(26., 30.)];
    let dirt_patches = [
        Vec3::new(-250., 0., 2.),
        Vec3::new(250., 0., 2.),
        Vec3::new(0., 250., 2.),
        Vec3::new(0., -250., 2.),
        Vec3::new(176., 176., 2.),
        Vec3::new(-176., 176., 2.),
        Vec3::new(-176., -176., 2.),
        Vec3::new(176., -176., 2.),
    ];
    for i in 0..8 {
        commands.spawn((
            Name::new("Game Object"),
            Transform::from_translation(dirt_patches[i]).with_scale(Vec3::new(1.5, 1.5, 1.)),
            StateScoped(Screen::Gameplay),
            ServerGameObject(0),
        ));
    }

    commands.spawn((
        Name::new("Pond"),
        Transform::from_translation(Vec2::ZERO.extend(2.)).with_scale(Vec3::new(1.5, 1.5, 1.)),
        StateScoped(Screen::Gameplay),
        Collider {
            size: obj_collider_sizes[1],
            collides_with_player: true,
            collides_with_projectile: false,
        },
        ServerGameObject(1),
    ));

    let num_trees = fastrand::usize(12..=20);

    for _ in 0..num_trees {
        let mut rng = rand::thread_rng();

        // Generate a random angle between 0 and 2*pi
        let angle = rng.gen_range(0.0..std::f32::consts::PI * 2.0);

        // Generate a random distance greater than the minimum radius (e.g., 250)
        let distance = rng.gen_range(270.0..500.0); // You can adjust the upper bound here

        // Convert polar coordinates to Cartesian coordinates (x, y)
        let x = distance * angle.cos();
        let y = distance * angle.sin();
        commands.spawn((
            Name::new("Tree"),
            Transform::from_translation(Vec3::new(x, y, 3.)).with_scale(Vec3::new(1.5, 1.5, 1.)),
            StateScoped(Screen::Gameplay),
            Collider {
                size: obj_collider_sizes[2],
                collides_with_player: true,
                collides_with_projectile: true,
            },
            ServerGameObject(2),
        ));
    }
    let num_walls = 8; //fastrand::usize(4..=6);

    let wall_base_pos = [
        Vec3::new(-300., 0., 3.),
        Vec3::new(300., 0., 3.),
        Vec3::new(0., 300., 3.),
        Vec3::new(0., -300., 3.),
        Vec3::new(212., 212., 3.),
        Vec3::new(-212., 212., 3.),
        Vec3::new(-212., -212., 3.),
        Vec3::new(212., -212., 3.),
    ];
    // Keep walls clear of player spawn pads so ducks don't start inside colliders.
    const SPAWN_CLEAR_RADIUS: f32 = 100.0;
    println!("SPAWNING WALLS");
    for i in 0..num_walls {
        let mut rng = rand::thread_rng();
        let wall_type = rng.gen_range(0..=3);
        let size = match wall_type {
            0 => Vec2::new(64., 48.),
            1 => Vec2::new(94., 48.),
            2 => Vec2::new(32., 80.),
            _ => Vec2::new(32., 114.),
        };
        let collider_size = size * 1.5;
        let half = collider_size / 2.0;

        let mut pos = None;
        for _ in 0..24 {
            let x_offset = rng.gen_range(1.0..2.5);
            let y_offset = rng.gen_range(1.0..1.4);
            let candidate = wall_base_pos[i] * Vec3::new(x_offset, y_offset, 1.);
            let overlaps_spawn = SPAWN_POSITIONS.iter().any(|spawn| {
                let dx = (candidate.x - spawn.x).abs();
                let dy = (candidate.y - spawn.y).abs();
                dx < half.x + SPAWN_CLEAR_RADIUS && dy < half.y + SPAWN_CLEAR_RADIUS
            });
            if !overlaps_spawn {
                pos = Some(candidate);
                break;
            }
        }
        let pos = pos.unwrap_or_else(|| {
            // Last resort: push outward from the origin so walls stay off spawn pads.
            let dir = wall_base_pos[i]
                .truncate()
                .normalize_or_zero();
            let distance = 420.0 + half.x.max(half.y);
            (dir * distance).extend(wall_base_pos[i].z)
        });

        commands.spawn((
            Name::new("Wall"),
            Transform::from_translation(pos).with_scale(Vec3::new(1.5, 1.5, 1.)),
            StateScoped(Screen::Gameplay),
            Collider {
                size: collider_size,
                collides_with_player: true,
                collides_with_projectile: true,
            },
            ServerGameObject(3 + wall_type),
        ));
    }
}

fn spawn_coins(
    mut commands: Commands,
    time: Res<Time>,
    mut spawner: ResMut<CoinSpawner>,
    mut server: ResMut<RenetServer>,
) {
    if spawner.timer.tick(time.delta()).just_finished() {
        let mut rng = rand::thread_rng();
        let x_offset = rng.gen_range(-750.0..750.0); // You can adjust the upper bound here
        let y_offset = rng.gen_range(-400.0..400.0); // You can adjust the upper bound here
        let pos = Vec3::new(x_offset, y_offset, 3.);
        spawn_coin(&mut commands, &mut server, pos);
    }
}
