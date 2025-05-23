//! Demo gameplay. All of these modules are only intended for demonstration
//! purposes and should be replaced with your own game logic.
//! Feel free to change the logic found here if you feel like tinkering around
//! to get a feeling for the template.

use bevy::prelude::*;

pub mod animation;
pub mod client;
pub mod level;
pub mod lib;
pub mod movement;
pub mod physics;
pub mod player;
pub mod projectile;

pub(super) fn plugin(app: &mut App) {
    app.add_plugins((
        physics::plugin,
        movement::plugin,
        client::plugins,
        animation::plugin,
        player::plugin,
        level::plugin,
        projectile::plugin,
    ));
}
