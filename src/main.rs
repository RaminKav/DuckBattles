// Disable console on Windows for non-dev builds.
/*
Duck Battles

Gather lilly pads (or some other currency).

Lilly pads spawn fairly quickly around the map, first player to grab it gets it.

The map will have some obstacles to hide behind, and it will be a closed map.

As you get more lilly pads, you grow in size, becomming easier to hit. Perhapse you also move slower

You can fire a projectile every once in a while, and if you hit someone, some of
their lilly pads will scatter around the map, being available for pick-up by anyone.

When the timer runs out, the player with the most lilly pads wins.


*/
#![cfg_attr(not(feature = "dev"), windows_subsystem = "windows")]

use bevy::prelude::*;
use chexy_butt_balloons::AppPlugin;

fn main() -> AppExit {
    App::new().add_plugins(AppPlugin).run()
}
