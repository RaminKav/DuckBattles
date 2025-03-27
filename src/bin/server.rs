use std::{collections::HashMap, f32::consts::PI};

use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::*,
};
use bevy_egui::{EguiContexts, EguiPlugin};
use bevy_renet::{
    renet::{ClientId, RenetServer, ServerEvent},
    RenetServerPlugin,
};
use chexy_butt_balloons::{
    demo::{
        animation::FacingDirection,
        lib::{
            connection_config, ClientChannel, NetworkedEntities, PlayerCommand, PlayerInput,
            ServerChannel, ServerMessages, Velocity, PROTOCOL_ID,
        },
        movement::{apply_movement, apply_screen_wrap, MovementController},
        physics::{check_collision, Collider},
        player::PlayerAssets,
    },
    screens::Screen,
    AppSet,
};

use rand::Rng;
use renet_visualizer::RenetServerVisualizer;

#[derive(Component)]
pub struct ServerGameObject(pub u64);

#[derive(Debug, Default, Resource)]
pub struct ServerLobby {
    pub players: HashMap<ClientId, Entity>,
}

const PLAYER_MOVE_SPEED: f32 = 300.0;
const PROJECTILE_MOVE_SPEED: f32 = 350.0;
const SPAWN_POSITIONS: [Vec3; 8] = [
    Vec3::new(-250., 0., 2.),
    Vec3::new(250., 0., 2.),
    Vec3::new(0., 250., 2.),
    Vec3::new(0., -250., 2.),
    Vec3::new(176., 176., 2.),
    Vec3::new(-176., 176., 2.),
    Vec3::new(-176., -176., 2.),
    Vec3::new(176., -176., 2.),
];

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default, Reflect)]
#[reflect(Component)]
pub struct Player {
    pub id: ClientId,
}

#[derive(Debug, Component)]
struct Bot {
    auto_cast: Timer,
}

#[derive(Debug, Resource)]
struct BotId(u64);

#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
#[reflect(Component)]
pub struct Projectile {
    pub speed: f32,
    pub direction: Vec2,
}

// #[cfg(feature = "netcode")]
fn add_netcode_network(app: &mut App) {
    use bevy_renet::netcode::{
        NetcodeServerPlugin, NetcodeServerTransport, ServerAuthentication, ServerConfig,
    };
    use std::{net::UdpSocket, time::SystemTime};

    app.add_plugins(NetcodeServerPlugin);

    let server = RenetServer::new(connection_config());

    let public_addr = "127.0.0.1:5000".parse().unwrap();
    let socket = UdpSocket::bind(public_addr).unwrap();
    let current_time: std::time::Duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let server_config = ServerConfig {
        current_time,
        max_clients: 64,
        protocol_id: PROTOCOL_ID,
        public_addresses: vec![public_addr],
        authentication: ServerAuthentication::Unsecure,
    };

    let transport = NetcodeServerTransport::new(server_config, socket).unwrap();
    app.insert_resource(server);
    app.insert_resource(transport);
}

#[cfg(feature = "steam")]
fn add_steam_network(app: &mut App) {
    use bevy_renet::steam::{
        AccessPermission, SteamServerConfig, SteamServerPlugin, SteamServerTransport,
    };
    use demo_bevy::connection_config;
    use steamworks::SingleClient;

    let (steam_client, single) = steamworks::Client::init_app(480).unwrap();

    let server: RenetServer = RenetServer::new(connection_config());

    let steam_transport_config = SteamServerConfig {
        max_clients: 10,
        access_permission: AccessPermission::Public,
    };
    let transport = SteamServerTransport::new(&steam_client, steam_transport_config).unwrap();

    app.add_plugins(SteamServerPlugin);
    app.insert_resource(server);
    app.insert_non_send_resource(transport);
    app.insert_non_send_resource(single);

    fn steam_callbacks(client: NonSend<SingleClient>) {
        client.run_callbacks();
    }

    app.add_systems(PreUpdate, steam_callbacks);
}

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins);

    app.add_plugins(RenetServerPlugin);
    app.add_plugins(FrameTimeDiagnosticsPlugin);
    app.add_plugins(LogDiagnosticsPlugin::default());
    app.add_plugins(EguiPlugin);

    app.insert_resource(ServerLobby::default());
    app.insert_resource(BotId(0));

    app.insert_resource(RenetServerVisualizer::<200>::default());

    // #[cfg(feature = "netcode")]
    add_netcode_network(&mut app);

    #[cfg(feature = "steam")]
    add_steam_network(&mut app);

    app.add_systems(
        Update,
        (
            server_update_system,
            server_network_sync,
            move_players_system,
            update_visulizer_system,
            spawn_bot,
            bot_autocast,
        ),
    );
    app.add_systems(
        Update,
        (apply_movement, apply_screen_wrap)
            .chain()
            .in_set(AppSet::Update),
    );

    // app.add_systems(FixedUpdate, move_projectiles);
    app.add_systems(Startup, generate_world);

    app.add_systems(PostUpdate, projectile_on_removal_system);

    // app.add_systems(Startup, setup_simple_camera);

    app.run();
}

#[allow(clippy::too_many_arguments)]
fn server_update_system(
    mut server_events: EventReader<ServerEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut lobby: ResMut<ServerLobby>,
    mut server: ResMut<RenetServer>,
    mut visualizer: ResMut<RenetServerVisualizer<200>>,
    players: Query<(Entity, &Player, &Transform, &MovementController)>,
    game_objects: Query<(&Transform, &ServerGameObject)>,
) {
    for event in server_events.read() {
        match event {
            ServerEvent::ClientConnected { client_id } => {
                println!("Player {} connected.", client_id);
                visualizer.add_client(*client_id);

                // Initialize other players for this new client
                for (entity, player, transform, _) in players.iter() {
                    let translation: [f32; 3] = transform.translation.into();
                    let message = bincode::serialize(&ServerMessages::PlayerCreate {
                        id: player.id,
                        entity,
                        translation,
                    })
                    .unwrap();
                    server.send_message(*client_id, ServerChannel::ServerMessages, message);
                }

                // Initialize game objects for this player
                for (transform, id) in game_objects.iter() {
                    let translation: [f32; 3] = transform.translation.into();
                    let message = bincode::serialize(&ServerMessages::SpawnGameObject {
                        id: id.0,
                        translation,
                    })
                    .unwrap();
                    server.send_message(*client_id, ServerChannel::ServerMessages, message);
                }
                // Spawn new player
                let transform = Transform::from_translation(
                    SPAWN_POSITIONS[lobby.players.len() % SPAWN_POSITIONS.len()],
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
                        size: Vec2::new(14., 24.),
                        collides_with_player: true,
                        collides_with_projectile: true,
                    })
                    .insert(PlayerInput::default())
                    .insert(Velocity::default())
                    .insert(Player { id: *client_id })
                    .id();

                lobby.players.insert(*client_id, player_entity);

                let translation: [f32; 3] = transform.translation.into();
                let message = bincode::serialize(&ServerMessages::PlayerCreate {
                    id: *client_id,
                    entity: player_entity,
                    translation,
                })
                .unwrap();
                server.broadcast_message(ServerChannel::ServerMessages, message);
            }
            ServerEvent::ClientDisconnected { client_id, reason } => {
                println!("Player {} disconnected: {}", client_id, reason);
                visualizer.remove_client(*client_id);
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
                PlayerCommand::BasicAttack => {
                    println!("Received basic attack from client {}", client_id);

                    if let Some(player_entity) = lobby.players.get(&client_id) {
                        if let Ok((_, _, player_transform, player_movement)) =
                            players.get(*player_entity)
                        {
                            let player_dir = player_movement.intent;
                            if player_dir == Vec2::ZERO {
                                continue;
                            }
                            let angle =
                                player_dir.y.atan2(player_dir.x) - std::f32::consts::PI / 2.0;

                            let offset_distance = 50.0; // How far in front of the player to spawn the projectile
                            let offset = player_dir * offset_distance;
                            let spawn_position = player_transform.translation.xy() + offset;

                            let final_translation = player_transform
                                .with_translation(spawn_position.extend(10.))
                                .translation;

                            let projectile_entity = commands
                                .spawn((
                                    Mesh2d(meshes.add(Rectangle::new(1.0, 8.0))),
                                    MeshMaterial2d(materials.add(Color::srgb(1.0, 0.0, 0.0))),
                                    Transform::from_translation(final_translation)
                                        .with_rotation(Quat::from_rotation_z(angle)),
                                ))
                                .insert(Collider {
                                    size: Vec2::new(12., 18.),
                                    collides_with_player: true,
                                    collides_with_projectile: true,
                                })
                                .insert(FacingDirection(player_dir))
                                .insert(Projectile {
                                    speed: PROJECTILE_MOVE_SPEED,
                                    direction: player_dir,
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
            }
        }
        while let Some(message) = server.receive_message(client_id, ClientChannel::Input) {
            let input: PlayerInput = bincode::deserialize(&message).unwrap();

            if let Some(player_entity) = lobby.players.get(&client_id) {
                // println!("INPUT! {:?}", input);
                commands.entity(*player_entity).insert(input);
            }
        }
    }
}

fn update_visulizer_system(
    mut egui_contexts: EguiContexts,
    mut visualizer: ResMut<RenetServerVisualizer<200>>,
    server: Res<RenetServer>,
) {
    visualizer.update(&server);
    visualizer.show_window(egui_contexts.ctx_mut());
}

#[allow(clippy::type_complexity)]
fn server_network_sync(
    mut server: ResMut<RenetServer>,
    query: Query<
        (Entity, &Transform, Option<&FacingDirection>),
        Or<(With<Player>, With<Projectile>)>,
    >,
) {
    let mut networked_entities = NetworkedEntities::default();
    for (entity, transform, maybe_direction) in query.iter() {
        networked_entities.entities.push(entity);
        networked_entities
            .translations
            .push(transform.translation.into());
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
    mut commands: Commands,
    mut query: Query<(Entity, &mut MovementController, &PlayerInput)>,
) {
    for (e, mut controller, input) in query.iter_mut() {
        let x = (input.right as i8 - input.left as i8) as f32;
        let y = (input.up as i8 - input.down as i8) as f32;
        let direction = Vec2::new(x, y).normalize_or_zero();
        // velocity.0.x = direction.x * PLAYER_MOVE_SPEED;
        // velocity.0.z = direction.y * PLAYER_MOVE_SPEED;
        controller.intent = direction;
        commands.entity(e).insert(FacingDirection(direction));
    }
}

fn move_projectiles(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &Projectile, &mut Transform, &Collider), With<Projectile>>,
    colliders: Query<(&Transform, &Collider), Without<Projectile>>,
) {
    for (e, projectile, mut proj_transform, proj_collider) in &mut query {
        let movement_this_frame =
            projectile.direction.extend(0.0) * projectile.speed * time.delta_secs();
        for (collider_transform, collider) in &colliders {
            //use check_collision

            if collider.collides_with_player
                && check_collision(
                    &(proj_transform.translation + movement_this_frame),
                    proj_collider,
                    &collider_transform.translation,
                    collider,
                )
            {
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
        let message = ServerMessages::DespawnProjectile { entity };
        let message = bincode::serialize(&message).unwrap();

        server.broadcast_message(ServerChannel::ServerMessages, message);
    }
}

fn spawn_bot(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut lobby: ResMut<ServerLobby>,
    mut server: ResMut<RenetServer>,
    mut bot_id: ResMut<BotId>,
    mut commands: Commands,
) {
    if keyboard_input.just_pressed(KeyCode::KeyB) {
        let client_id: ClientId = bot_id.0;
        bot_id.0 += 1;
        // Spawn new player
        let transform = Transform::from_xyz(
            (fastrand::f32() - 0.5) * 300.,
            (fastrand::f32() - 0.5) * 600.,
            8.,
        );
        let player_entity = commands
            .spawn((
                Mesh3d(meshes.add(Mesh::from(Capsule3d::default()))),
                MeshMaterial3d(materials.add(Color::srgb(0.8, 0.7, 0.6))),
                transform,
            ))
            .insert(Player { id: client_id })
            .insert(Bot {
                auto_cast: Timer::from_seconds(3.0, TimerMode::Repeating),
            })
            .id();

        lobby.players.insert(client_id, player_entity);

        let translation: [f32; 3] = transform.translation.into();
        let message = bincode::serialize(&ServerMessages::PlayerCreate {
            id: client_id,
            entity: player_entity,
            translation,
        })
        .unwrap();
        server.broadcast_message(ServerChannel::ServerMessages, message);
    }
}

fn bot_autocast(
    time: Res<Time>,
    mut server: ResMut<RenetServer>,
    mut bots: Query<(&Transform, &mut Bot), With<Player>>,
    mut commands: Commands,
) {
    for (transform, mut bot) in &mut bots {
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
            .with_translation(spawn_position.extend(0.))
            .translation;

        let projectile_entity = commands
            .spawn((Transform::from_translation(final_translation)
                .with_rotation(Quat::from_rotation_z(angle)),))
            .insert(Projectile {
                speed: PROJECTILE_MOVE_SPEED,
                direction: bot_dir,
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

fn generate_world(mut commands: Commands) {
    let obj_collider_sizes = [Vec2::new(0., 0.), Vec2::new(90., 76.), Vec2::new(26., 30.)];
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

    let num_trees = fastrand::usize(16..=24);

    for _ in 0..num_trees {
        let mut rng = rand::thread_rng();

        // Generate a random angle between 0 and 2*pi
        let angle = rng.gen_range(0.0..std::f32::consts::PI * 2.0);

        // Generate a random distance greater than the minimum radius (e.g., 250)
        let distance = rng.gen_range(270.0..800.0); // You can adjust the upper bound here

        // Convert polar coordinates to Cartesian coordinates (x, y)
        let x = distance * angle.cos();
        let y = distance * angle.sin();
        commands.spawn((
            Name::new("Tree"),
            Transform::from_translation(Vec3::new(x, y, 2.)).with_scale(Vec3::new(1.5, 1.5, 1.)),
            StateScoped(Screen::Gameplay),
            Collider {
                size: obj_collider_sizes[2],
                collides_with_player: true,
                collides_with_projectile: true,
            },
            ServerGameObject(2),
        ));
    }
}
