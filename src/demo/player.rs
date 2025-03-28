//! Plugin handling the player character in particular.
//! Note that this is separate from the `movement` module as that could be used
//! for other characters as well.

use bevy::{
    ecs::world::Command,
    image::{ImageLoaderSettings, ImageSampler},
    prelude::*,
};
use bevy_renet::renet::ClientId;

use crate::{
    asset_tracking::LoadResource, demo::animation::PlayerAnimation, screens::Screen, AppSet,
};

pub(super) fn plugin(app: &mut App) {
    app.load_resource::<PlayerAssets>();

    // Record directional input as movement controls.
    // app.add_systems(
    //     Update,
    //     record_player_directional_input.in_set(AppSet::RecordInput),
    // );
}

#[derive(Component, Debug, Reflect)]
#[reflect(Component)]
pub struct Coin {
    pub claimed_by: Option<Entity>,
}

/// A command to spawn the player character.
#[derive(Debug)]
pub struct SpawnPlayer {
    /// See [`MovementController::max_speed`].
    pub max_speed: f32,
}

impl Command for SpawnPlayer {
    fn apply(self, world: &mut World) {
        let _ = world.run_system_cached_with(spawn_player, self);
    }
}

fn spawn_player(
    In(config): In<SpawnPlayer>,
    mut commands: Commands,
    player_assets: Res<PlayerAssets>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    // A texture atlas is a way to split one image with a grid into multiple
    // sprites. By attaching it to a [`SpriteBundle`] and providing an index, we
    // can specify which section of the image we want to see. We will use this
    // to animate our player character. You can learn more about texture atlases in
    // this example: https://github.com/bevyengine/bevy/blob/latest/examples/2d/texture_atlas.rs
    let layout = TextureAtlasLayout::from_grid(UVec2::splat(32), 6, 2, Some(UVec2::splat(1)), None);
    let texture_atlas_layout = texture_atlas_layouts.add(layout);
    let player_animation = PlayerAnimation::new();

    commands.spawn((
        Name::new("Player"),
        Sprite {
            image: player_assets.ducky.clone(),
            texture_atlas: Some(TextureAtlas {
                layout: texture_atlas_layout.clone(),
                index: player_animation.get_atlas_index(),
            }),
            ..default()
        },
        Transform::from_scale(Vec2::splat(2.0).extend(1.0)),
        player_animation,
        StateScoped(Screen::Gameplay),
    ));
}

// fn record_player_directional_input(
//     input: Res<ButtonInput<KeyCode>>,
//     mut controller_query: Query<&mut MovementController, With<Player>>,
// ) {
//     // Collect directional input.
//     let mut intent = Vec2::ZERO;
//     if input.pressed(KeyCode::KeyW) || input.pressed(KeyCode::ArrowUp) {
//         intent.y += 1.0;
//     }
//     if input.pressed(KeyCode::KeyS) || input.pressed(KeyCode::ArrowDown) {
//         intent.y -= 1.0;
//     }
//     if input.pressed(KeyCode::KeyA) || input.pressed(KeyCode::ArrowLeft) {
//         intent.x -= 1.0;
//     }
//     if input.pressed(KeyCode::KeyD) || input.pressed(KeyCode::ArrowRight) {
//         intent.x += 1.0;
//     }

//     // Normalize so that diagonal movement has the same speed as
//     // horizontal and vertical movement.
//     // This should be omitted if the input comes from an analog stick instead.
//     let intent = intent.normalize_or_zero();

// }

#[derive(Resource, Asset, Reflect, Clone)]
pub struct PlayerAssets {
    // This #[dependency] attribute marks the field as a dependency of the Asset.
    // This means that it will not finish loading until the labeled asset is also loaded.
    #[dependency]
    pub ducky: Handle<Image>,
    #[dependency]
    pub coin: Handle<Image>,
    #[dependency]
    pub dirt_patch: Handle<Image>,
    #[dependency]
    pub map: Handle<Image>,
    #[dependency]
    pub pond: Handle<Image>,
    #[dependency]
    pub trees: Handle<Image>,
    #[dependency]
    pub bullet: Handle<Image>,
    #[dependency]
    pub wall_h_small: Handle<Image>,
    #[dependency]
    pub wall_h_large: Handle<Image>,
    #[dependency]
    pub wall_v_small: Handle<Image>,
    #[dependency]
    pub wall_v_large: Handle<Image>,
    #[dependency]
    pub steps: Vec<Handle<AudioSource>>,
}

impl PlayerAssets {
    pub const PATH_DUCKY: &'static str = "images/ducky.png";
    pub const PATH_BULLET: &'static str = "images/bullet.png";
    pub const PATH_WALL_H_SMALL: &'static str = "images/wall_h_small.png";
    pub const PATH_WALL_V_SMALL: &'static str = "images/wall_v_small.png";
    pub const PATH_WALL_V_LARGE: &'static str = "images/wall_v_large.png";
    pub const PATH_WALL_H_LARGE: &'static str = "images/wall_h_large.png";
    pub const PATH_COIN: &'static str = "images/coin.png";
    pub const PATH_DIRT_PATCH: &'static str = "images/dirt_patch.png";
    pub const PATH_MAP: &'static str = "images/map.png";
    pub const PATH_POND: &'static str = "images/pond.png";
    pub const PATH_TREES: &'static str = "images/trees.png";
    pub const PATH_STEP_1: &'static str = "audio/sound_effects/step1.ogg";
    pub const PATH_STEP_2: &'static str = "audio/sound_effects/step2.ogg";
    pub const PATH_STEP_3: &'static str = "audio/sound_effects/step3.ogg";
    pub const PATH_STEP_4: &'static str = "audio/sound_effects/step4.ogg";
}

impl FromWorld for PlayerAssets {
    fn from_world(world: &mut World) -> Self {
        let assets = world.resource::<AssetServer>();
        Self {
            ducky: assets.load_with_settings(
                PlayerAssets::PATH_DUCKY,
                |settings: &mut ImageLoaderSettings| {
                    // Use `nearest` image sampling to preserve the pixel art style.
                    settings.sampler = ImageSampler::nearest();
                },
            ),
            wall_h_small: assets.load(PlayerAssets::PATH_WALL_H_SMALL),
            wall_h_large: assets.load(PlayerAssets::PATH_WALL_H_LARGE),
            wall_v_small: assets.load(PlayerAssets::PATH_WALL_V_SMALL),
            wall_v_large: assets.load(PlayerAssets::PATH_WALL_V_LARGE),
            bullet: assets.load(PlayerAssets::PATH_BULLET),
            coin: assets.load(PlayerAssets::PATH_COIN),
            dirt_patch: assets.load(PlayerAssets::PATH_DIRT_PATCH),
            map: assets.load(PlayerAssets::PATH_MAP),
            pond: assets.load(PlayerAssets::PATH_POND),
            trees: assets.load(PlayerAssets::PATH_TREES),
            steps: vec![
                assets.load(PlayerAssets::PATH_STEP_1),
                assets.load(PlayerAssets::PATH_STEP_2),
                assets.load(PlayerAssets::PATH_STEP_3),
                assets.load(PlayerAssets::PATH_STEP_4),
            ],
        }
    }
}
