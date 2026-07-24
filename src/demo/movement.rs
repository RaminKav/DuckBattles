//! Handle player input and translate it into movement through a character
//! controller. A character controller is the collection of systems that govern
//! the movement of characters.
//!
//! In our case, the character controller has the following logic:
//! - Set [`MovementController`] intent based on directional keyboard input.
//!   This is done in the `player` module, as it is specific to the player
//!   character.
//! - Apply movement based on [`MovementController`] intent and maximum speed.
//! - Wrap the character within the window.
//!
//! Note that the implementation used here is limited for demonstration
//! purposes. If you want to move the player in a smoother way,
//! consider using a [fixed timestep](https://github.com/bevyengine/bevy/blob/main/examples/movement/physics_in_fixed_timestep.rs).

use bevy::prelude::*;

use crate::{
    screens::{gameplay::ScoreEvent, Screen},
    AppSet,
};

use super::{
    lib::Player,
    physics::{check_collision, Collider},
    player::Coin,
};

/// Map sprite is 1024×800 at scale 1.5 → playable world half-extents.
pub const MAP_WORLD_HALF: Vec2 = Vec2::new(768.0, 600.0);
/// Keep player bodies slightly inside the map edge.
pub const PLAYER_BOUNDS_MARGIN: f32 = 28.0;

pub fn plugin(app: &mut App) {
    app.register_type::<MovementController>();
    app.add_systems(
        Update,
        (apply_movement, apply_map_bounds)
            .chain()
            .in_set(AppSet::Update)
            .run_if(in_state(Screen::Gameplay)),
    );
}

/// These are the movement parameters for our character controller.
/// For now, this is only used for a single player, but it could power NPCs or
/// other players as well.
#[derive(Component, Reflect)]
#[reflect(Component)]
pub struct MovementController {
    /// The direction the character wants to move in.
    pub intent: Vec2,

    /// Maximum speed in world units per second.
    /// 1 world unit = 1 pixel when using the default 2D camera and no physics
    /// engine.
    pub max_speed: f32,

    /// Temporary multiplier (e.g. dash burst).
    pub speed_multiplier: f32,
}

impl Default for MovementController {
    fn default() -> Self {
        Self {
            intent: Vec2::ZERO,
            // 400 pixels per second is a nice default, but we can still vary this per character.
            max_speed: 400.0,
            speed_multiplier: 1.0,
        }
    }
}

pub fn apply_movement(
    mut commands: Commands,
    time: Res<Time>,
    mut score_event: EventWriter<ScoreEvent>,
    mut movement_query: Query<(Entity, &MovementController)>,
    mut colliders: Query<(Entity, &mut Transform, &Collider, Option<&Coin>)>,
) {
    let mut movement_data: Vec<_> = vec![];
    for (entity, controller) in &mut movement_query {
        let velocity =
            controller.max_speed * controller.speed_multiplier * controller.intent;
        let movement_this_frame = velocity.extend(0.0) * time.delta_secs();
        let (_, t, c, _) = colliders.get(entity).unwrap();
        movement_data.push((entity, t.clone(), c.clone(), movement_this_frame));
        // println!("num movers: {:?}", movement_data.len());
    }

    'outer: for (entity, mover_transform, mover_collider, movement_this_frame) in movement_data {
        let mut mover_mask = Vec3::ONE;
        // Feet/body offset used for player collision checks.
        let body_pos = mover_transform.translation - Vec3::new(0., 10., 0.);
        for (collider_entity, collider_transform, collider, maybe_coin) in colliders.iter_mut() {
            if collider_entity == entity {
                // Don't check collision with self.
                continue;
            }
            if !collider.collides_with_player {
                continue;
            }

            let wall_pos = collider_transform.translation;
            let already_inside =
                check_collision(&body_pos, &mover_collider, &wall_pos, collider);

            if check_collision(
                &(body_pos + movement_this_frame * Vec3::new(1., 0., 1.)),
                &mover_collider,
                &wall_pos,
                collider,
            ) {
                if maybe_coin.is_some() {
                    score_event.send(ScoreEvent {
                        player: entity,
                        delta: 1,
                    });
                    commands.entity(collider_entity).despawn();
                    continue;
                } else if !already_inside {
                    // Only block when approaching from outside; allow escape if stuck inside.
                    mover_mask.x = 0.;
                }
            }

            if check_collision(
                &(body_pos + movement_this_frame * Vec3::new(0., 1., 1.)),
                &mover_collider,
                &wall_pos,
                collider,
            ) {
                if maybe_coin.is_some() {
                    score_event.send(ScoreEvent {
                        player: entity,
                        delta: 1,
                    });
                    commands.entity(collider_entity).despawn();
                    continue;
                } else if !already_inside {
                    mover_mask.y = 0.;
                }
            }

            // No movement possible.
            if mover_mask == Vec3::ZERO {
                continue 'outer;
            }
        }
        let mut transform = colliders.get_mut(entity).unwrap().1;
        transform.translation += movement_this_frame * mover_mask;
    }
    // for (entity, controller) in movement_query.iter_mut() {
    //     let velocity = controller.max_speed * controller.intent;
    //     let movement_this_frame = velocity.extend(0.0) * time.delta_secs();
    //     transform.translation += movement_this_frame;
    // }
}

/// Clamp players to the map's world bounds (shared constant — not window size).
/// Reliable across screen sizes because the game world is fixed; the camera just
/// shows more/less of that same map.
pub fn apply_map_bounds(mut query: Query<&mut Transform, With<Player>>) {
    let half = MAP_WORLD_HALF - Vec2::splat(PLAYER_BOUNDS_MARGIN);
    for mut transform in &mut query {
        transform.translation.x = transform.translation.x.clamp(-half.x, half.x);
        transform.translation.y = transform.translation.y.clamp(-half.y, half.y);
    }
}
