use std::collections::HashMap;
use std::time::UNIX_EPOCH;

use crate::demo::animation::{FacingDirection, PlayerAnimation};

use crate::demo::lib::connection_config;
use crate::demo::physics::Collider;
use crate::screens::gameplay::{calculate_score_growth, ScoreText};
use crate::screens::lobby::ToggleReadyEvent;
use crate::screens::Screen;
use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::Vec3,
    prelude::*,
};
use bevy_mod_reqwest::{BevyReqwest, JsonResponse, ReqwestErrorEvent, ReqwestResponseEvent};
use renet2_netcode::{
     ClientSocket, NativeSocket, NetcodeClientTransport, ServerCertHash, WebServerDestination
};

use bevy_egui::{EguiContexts, EguiPlugin};

use bevy_renet2::prelude::{
    client_connected, ClientId, ConnectionConfig, RenetClient, RenetClientPlugin,
};
use renet2_visualizer::{RenetClientVisualizer, RenetVisualizerStyle};

use super::lib::{
    ClientChannel, NetworkedEntities, Player, PlayerCommand, PlayerInput, ServerChannel,
    ServerMessages,
};
use super::player::PlayerAssets;

#[derive(Component)]
struct ControlledPlayer;

#[derive(Default, Resource)]
pub struct NetworkMapping(HashMap<Entity, Entity>);

#[derive(Debug)]
struct PlayerInfo {
    client_entity: Entity,
    server_entity: Entity,
}

#[derive(Debug, Default, Resource)]
pub struct ClientLobby {
    players: HashMap<ClientId, PlayerInfo>,
}

#[derive(Debug, Resource)]
pub struct CurrentClientId(u64);

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Connected;

pub const PLAYER_BASE_COLLIDER_SIZE: Vec2 = Vec2::new(14., 10.);

// #[cfg(feature = "netcode")]
 fn add_netcode_network(app: &mut App) {
    use super::lib::PROTOCOL_ID;
    use bevy_renet2::netcode::{
        ClientAuthentication, NetcodeClientPlugin, NetcodeClientTransport, NetcodeTransportError,
    };
    use std::{net::UdpSocket, time::SystemTime};

    app.add_plugins(NetcodeClientPlugin);

    app.configure_sets(Update, Connected.run_if(client_connected));

    // If any error is found we just panic
    #[allow(clippy::never_loop)]
    fn panic_on_error_system(mut renet_error: EventReader<NetcodeTransportError>) {
        for e in renet_error.read() {
            panic!("{}", e);
        }
    }
    #[cfg(target_family = "wasm")]
    fn connect_wasm(mut client: BevyReqwest, mut commands: Commands) {
        use renet2_netcode::{
            webtransport_is_available_with_cert_hashes, ClientSocket, CongestionControl, NetcodeClientTransport, ServerCertHash, WebServerDestination, WebSocketClient, WebSocketClientConfig, WebTransportClient, WebTransportClientConfig
        };

        let url = "https://bored-api.appbrewery.com/random";

        let reqwest_request = client.get(url).build().unwrap();

        client
            .send(reqwest_request)
            .on_json_response(
                |trigger: Trigger<
                    JsonResponse<(WebServerDestination, ServerCertHash, url::Url)>,
                >| {
                    let (wt_server_dest, wt_server_cert_hash, ws_server_url) = trigger.event().0;
                    let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let connection_config = ConnectionConfig::test();
        let (client, transport, client_id) = match webtransport_is_available_with_cert_hashes() {
            true => {
                tracing::info!("setting up webtransport client (server = {:?})", wt_server_dest);

                let client_id = current_time.as_millis() as u64;
                let client_auth = ClientAuthentication::Unsecure {
                    client_id,
                    protocol_id: 0,
                    socket_id: 1, //WebTransport socket id is 1 in this example
                    server_addr: wt_server_dest.clone().into(),
                    user_data: None,
                };
                let socket_config = WebTransportClientConfig {
                    server_dest: wt_server_dest.into(),
                    congestion_control: CongestionControl::default(),
                    server_cert_hashes: Vec::from([wt_server_cert_hash]),
                };
                let socket = WebTransportClient::new(socket_config);

                let client = RenetClient::new(connection_config, socket.is_reliable());
                let transport = NetcodeClientTransport::new(current_time, client_auth, socket).unwrap();

                (client, transport, client_id)
            }
            false => {
                tracing::warn!("webtransport with cert hashes is not supported on this platform, falling back \
                    to websockets");
                tracing::info!("setting up websocket client (server = {:?})", ws_server_url.as_str());
                let socket_config = WebSocketClientConfig {
                    server_url: ws_server_url,
                };

                let socket = WebSocketClient::new(socket_config).unwrap();
                let client = RenetClient::new(connection_config, socket.is_reliable());
                let client_id = current_time.as_millis() as u64;

                let client_auth = ClientAuthentication::Unsecure {
                    client_id,
                    protocol_id: 0,
                    socket_id: 2, //WebSocket socket id is 2 in this example
                    server_addr: socket.server_address(),
                    user_data: None,
                };
                let transport = NetcodeClientTransport::new(current_time, client_auth, socket).unwrap();

                (client, transport, client_id)
            }
        };
        commands.insert_resource(transport);
        commands.insert_resource(client);
    
        commands.insert_resource(CurrentClientId(client_id));
                },
            )
            // In case of request error, it can be reached using an observersystem as well
            .on_error(|trigger: Trigger<ReqwestErrorEvent>| {
                let e = &trigger.event().0;
                bevy::log::info!("error: {e:?}");
            });
    }
    #[cfg(not(target_family = "wasm"))]
    fn connect_udp(mut commands: Commands) {
        println!("[CLIENT] Connecting to server...");
        let server_addr = "127.0.0.1:5000".parse().unwrap();
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();

        let client = RenetClient::new(connection_config(), false);

        let current_time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap();
        let client_id = current_time.as_millis() as u64;
        let authentication = ClientAuthentication::Unsecure {
            client_id,
            protocol_id: PROTOCOL_ID,
            socket_id: 0,
            server_addr,
            user_data: None,
        };


        let transport = NetcodeClientTransport::new(current_time, authentication, NativeSocket::new(socket).unwrap()).unwrap();
        commands.insert_resource(transport);
        commands.insert_resource(client);

        commands.insert_resource(CurrentClientId(client_id));
        println!("[CLIENT] Connected!");

    }
    app.add_systems(Update, panic_on_error_system);

    #[cfg(target_family = "wasm")]
    app.add_systems(
        Update,
        connect_wasm.run_if(in_state(Screen::Lobby).and(run_once)),
    );

    #[cfg(not(target_family = "wasm"))]
    app.add_systems(
        Update,
        connect_udp.run_if(in_state(Screen::Lobby).and(run_once)),
    );
}

pub(super) fn plugins(app: &mut App) {
    app.add_plugins(RenetClientPlugin);
    app.add_plugins(FrameTimeDiagnosticsPlugin);
    app.add_plugins(LogDiagnosticsPlugin::default());
    app.add_plugins(EguiPlugin);

    // #[cfg(feature = "netcode")]
    add_netcode_network(app);

    app.add_event::<PlayerCommand>();

    app.insert_resource(ClientLobby::default());
    app.insert_resource(PlayerInput::default());
    app.insert_resource(NetworkMapping::default());

    app.add_systems(Update, (player_input).run_if(in_state(Screen::Gameplay)));
    app.add_systems(Update, (player_read_input).run_if(in_state(Screen::Lobby)));
    app.add_systems(
        Update,
        (
            client_send_input,
            update_score_text,
            client_send_player_commands,
            client_sync_players,
        )
            .in_set(Connected),
    );

    app.insert_resource(RenetClientVisualizer::<200>::new(
        RenetVisualizerStyle::default(),
    ));

    // app.add_systems(Startup, (setup_target));
    app.add_systems(
        Update,
        update_visulizer_system.run_if(in_state(Screen::Gameplay)),
    );
}

fn update_visulizer_system(
    mut egui_contexts: EguiContexts,
    mut visualizer: ResMut<RenetClientVisualizer<200>>,
    client: Res<RenetClient>,
    mut show_visualizer: Local<bool>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
) {
    visualizer.add_network_info(client.network_info());
    if keyboard_input.just_pressed(KeyCode::F1) {
        *show_visualizer = !*show_visualizer;
    }
    if *show_visualizer {
        visualizer.show_window(egui_contexts.ctx_mut());
    }
}

fn player_input(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut player_input: ResMut<PlayerInput>,
    mut player_commands: EventWriter<PlayerCommand>,
) {
    player_input.left =
        keyboard_input.pressed(KeyCode::KeyA) || keyboard_input.pressed(KeyCode::ArrowLeft);
    player_input.right =
        keyboard_input.pressed(KeyCode::KeyD) || keyboard_input.pressed(KeyCode::ArrowRight);
    player_input.up =
        keyboard_input.pressed(KeyCode::KeyW) || keyboard_input.pressed(KeyCode::ArrowUp);
    player_input.down =
        keyboard_input.pressed(KeyCode::KeyS) || keyboard_input.pressed(KeyCode::ArrowDown);

    if keyboard_input.just_pressed(KeyCode::Space) {
        player_commands.send(PlayerCommand::BasicAttack);
    }
}
fn player_read_input(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut player_commands: EventWriter<PlayerCommand>,
) {
    if keyboard_input.just_pressed(KeyCode::Space) {
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
    mut toggles: EventWriter<ToggleReadyEvent>,
    mut next_screen: ResMut<NextState<Screen>>,
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
                    },
                    Sprite {
                        image: player_assets.ducky.clone(),
                        texture_atlas: Some(TextureAtlas {
                            layout: texture_atlas_layout.clone(),
                            index: player_animation.get_atlas_index(),
                        }),
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
                        size: Vec2::new(20., 24.),
                        collides_with_player: true,
                        collides_with_projectile: false,
                    },
                    Transform::from_translation(translation.into())
                        .with_scale(Vec3::new(1.5, 1.5, 1.)),
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
                        println!("SEND EVENT");
                        toggles.send(ToggleReadyEvent {
                            player: *client_entity,
                            is_ready,
                        });
                    }
                }
            }
            ServerMessages::StartGame => {
                println!("Starting game!");
                next_screen.set(Screen::Gameplay);
            }
        }
    }

    while let Some(message) = client.receive_message(ServerChannel::NetworkedEntities) {
        let networked_entities: NetworkedEntities = bincode::deserialize(&message).unwrap();
        for i in 0..networked_entities.entities.len() {
            if let Some(entity) = network_mapping.0.get(&networked_entities.entities[i]) {
                let translation = networked_entities.translations[i].into();
                let maybe_direction = networked_entities.facing_directions[i].map(Vec2::from_array);
                let mut transform = Transform {
                    translation,
                    ..Default::default()
                };
                if let Some(direction) = maybe_direction {
                    commands.entity(*entity).insert(FacingDirection(direction));
                }
                if let Some(score) = networked_entities.score[i] {
                    if let Ok(mut player) = player_data.get_mut(*entity) {
                        player.score = score;
                        transform.scale = Vec3::new(
                            1.0 + calculate_score_growth(score),
                            1.0 + calculate_score_growth(score),
                            1.0,
                        );
                    }
                }
                commands.entity(*entity).insert(transform);
            }
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
