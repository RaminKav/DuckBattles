//! A loading screen during which game assets are loaded.
//! This reduces stuttering, especially for audio on WASM.

use bevy::prelude::*;

use crate::{demo::lib::Player, screens::Screen};

pub(super) fn plugin(app: &mut App) {
    app.add_event::<ToggleReadyEvent>();
    app.add_systems(
        Update,
        (add_ready_checker, update_ready_checker).run_if(in_state(Screen::Lobby)),
    );
    app.add_systems(OnExit(Screen::Lobby), despawn_ready_checker);
}
const NOT_READY_COLOR: Color = Color::srgb(0.9, 0.1, 0.1);
const READY_COLOR: Color = Color::srgb(0.1, 0.9, 0.1);
#[derive(Component)]
pub struct ReadyTracker;

#[derive(Debug, Event)]
pub struct ToggleReadyEvent {
    pub player: Entity,
    pub is_ready: bool,
}

fn add_ready_checker(
    mut commands: Commands,
    new_players: Query<(Entity, &Player), Added<Player>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (entity, player) in new_players.iter() {
        commands
            .spawn((
                Mesh2d(meshes.add(Circle::new(10.0))),
                MeshMaterial2d(materials.add(if player.is_ready {
                    READY_COLOR
                } else {
                    NOT_READY_COLOR
                })),
                Transform::from_xyz(0., 20.0, 1.0),
            ))
            .insert(ReadyTracker)
            .set_parent(entity);
    }
}

fn update_ready_checker(
    mut toggles: EventReader<ToggleReadyEvent>,
    mut tracker_query: Query<(Entity, &Parent), With<ReadyTracker>>,
    mut commands: Commands,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for player in toggles.read() {
        println!("GOT EVENT: ");
        for (e, parent) in tracker_query.iter_mut() {
            println!("CHANGED PLAYER: {:?}", player.is_ready);
            if parent.get() == player.player {
                println!("CHANGING COLOR: {:?}", player.is_ready);
                commands
                    .entity(e)
                    .insert(MeshMaterial2d(materials.add(if player.is_ready {
                        READY_COLOR
                    } else {
                        NOT_READY_COLOR
                    })));
            }
        }
    }
}

fn despawn_ready_checker(mut commands: Commands, ready_query: Query<Entity, With<ReadyTracker>>) {
    for (ready_entity) in ready_query.iter() {
        commands.entity(ready_entity).despawn_recursive();
    }
}
