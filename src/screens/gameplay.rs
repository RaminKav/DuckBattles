//! The screen state for the main gameplay.

use std::process::CommandArgs;

use bevy::{input::common_conditions::input_just_pressed, prelude::*};
use bevy_renet2::prelude::RenetServer;

use crate::demo::client::PLAYER_BASE_COLLIDER_SIZE;
use crate::demo::lib::Player;
use crate::demo::lib::ServerChannel;
use crate::demo::lib::ServerMessages;
use crate::demo::physics::Collider;
use crate::demo::player;
use crate::demo::player::Coin;
use crate::theme::widgets::Containers;
use crate::theme::widgets::Widgets;
use crate::{
    asset_tracking::LoadResource, audio::Music, demo::level::spawn_level as spawn_level_command,
    screens::Screen,
};

pub(super) fn plugin(app: &mut App) {
    // app.add_systems(OnEnter(Screen::Gameplay), spawn_level);

    app.load_resource::<GameplayMusic>();
    app.add_systems(
        OnEnter(Screen::Lobby),
        (play_gameplay_music, spawn_score_text),
    );
    app.add_systems(OnExit(Screen::Gameplay), stop_music);
    app.add_event::<ScoreEvent>();
    app.add_systems(
        Update,
        return_to_title_screen
            .run_if(in_state(Screen::Gameplay).and(input_just_pressed(KeyCode::Escape))),
    );
}

fn spawn_level(mut commands: Commands) {
    commands.queue(spawn_level_command);
}

#[derive(Event)]
pub struct ScoreEvent {
    pub player: Entity,
    pub delta: i64,
}
#[derive(Resource, Asset, Reflect, Clone)]
pub struct GameplayMusic {
    #[dependency]
    handle: Handle<AudioSource>,
    entity: Option<Entity>,
}

impl FromWorld for GameplayMusic {
    fn from_world(world: &mut World) -> Self {
        let assets = world.resource::<AssetServer>();
        Self {
            handle: assets.load("audio/music/Fluffing A Duck.ogg"),
            entity: None,
        }
    }
}

fn play_gameplay_music(mut commands: Commands, mut music: ResMut<GameplayMusic>) {
    music.entity = Some(
        commands
            .spawn((
                AudioPlayer(music.handle.clone()),
                PlaybackSettings::LOOP,
                Music,
            ))
            .id(),
    );
}

fn stop_music(mut commands: Commands, mut music: ResMut<GameplayMusic>) {
    if let Some(entity) = music.entity.take() {
        commands.entity(entity).despawn_recursive();
    }
}

fn return_to_title_screen(mut next_screen: ResMut<NextState<Screen>>) {
    next_screen.set(Screen::Title);
}

#[derive(Component)]
pub struct ScoreText;

fn spawn_score_text(mut commands: Commands) {
    commands
        .ui_root()
        .insert(StateScoped(Screen::Gameplay))
        .with_children(|children| {
            children.label("Coins: 0").insert(ScoreText).insert(Node {
                position_type: PositionType::Absolute,
                left: Val::Px(10.0),
                top: Val::Px(10.0),

                ..default()
            });
        });
}

pub fn calculate_score_growth(score: i64) -> f32 {
    score as f32 * 0.1
}

pub fn handle_score_event(
    mut events: EventReader<ScoreEvent>,
    mut commands: Commands,
    mut player_query: Query<(Entity, &mut Transform, &mut Player)>,
) {
    for event in events.read() {
        if let Ok((entity, mut transform, mut player)) = player_query.get_mut(event.player) {
            player.score += event.delta;
            let score_growth = calculate_score_growth(player.score);
            transform.scale = Vec3::splat(score_growth);
            commands.entity(entity).insert(Collider {
                size: PLAYER_BASE_COLLIDER_SIZE * score_growth,
                collides_with_player: true,
                collides_with_projectile: true,
            });
            println!("Player {:?} score: {:?}", entity, player.score);
        }
    }
}

pub fn spawn_coin(
    commands: &mut Commands,
    server: &mut ResMut<RenetServer>,
    position: Vec3,
) -> Entity {
    let coin_entity = commands
        .spawn((
            Name::new("Coin"),
            Coin { claimed_by: None },
            Transform::from_translation(position).with_scale(Vec3::new(1.5, 1.5, 1.)),
            StateScoped(Screen::Gameplay),
            Collider {
                size: Vec2::new(20., 24.),
                collides_with_player: true,
                collides_with_projectile: false,
            },
        ))
        .id();
    let message = ServerMessages::SpawnCoin {
        entity: coin_entity,
        translation: position.into(),
    };
    let message = bincode::serialize(&message).unwrap();
    server.broadcast_message(ServerChannel::ServerMessages, message);

    coin_entity
}
