use bevy::prelude::*;

use crate::AppSet;

pub(super) fn plugin(app: &mut App) {
    app.register_type::<Projectile>();

    // Record directional input as movement controls.
    app.add_systems(Update, handle_move_projectiles.in_set(AppSet::Update));
}

#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
#[reflect(Component)]
pub struct Projectile {
    pub speed: f32,
    pub direction: Vec2,
}

// fn handle_projectile_input(
//     input: Res<ButtonInput<KeyCode>>,
//     mut commands: Commands,
//     player: Query<(Entity, &MovementController, &Transform)>,
//     mut meshes: ResMut<Assets<Mesh>>,
//     mut materials: ResMut<Assets<ColorMaterial>>,
//     mut player_commands: EventWriter<PlayerCommand>,
// ) {
//     let Ok((player_entity, player, player_txfm)) = player.get_single() else {
//         return;
//     };
//     let player_dir = player.intent;
//     if player_dir == Vec2::ZERO {
//         return;
//     }
//     // Collect directional input.
//     if input.just_pressed(KeyCode::Space) {
//         let color = Color::hsl(0.7, 0.95, 0.7);
//         let angle = player_dir.y.atan2(player_dir.x) - std::f32::consts::PI / 2.0;

//         let offset_distance = 50.0; // How far in front of the player to spawn the projectile
//         let offset = player_dir * offset_distance;
//         let spawn_position = player_txfm.translation.xy() + offset;
//         let spawn_position = player_txfm
//             .with_translation(spawn_position.extend(0.))
//             .translation;
//     }
// }

fn handle_move_projectiles(time: Res<Time>, mut query: Query<(&Projectile, &mut Transform)>) {
    for (projectile, mut transform) in &mut query {
        transform.translation +=
            projectile.direction.extend(0.0) * projectile.speed * time.delta_secs();
    }
}
