use bevy::prelude::*;

pub(super) fn plugin(_app: &mut App) {
    // No setup required for this plugin.
    // It's still good to have a function here so that we can add some setup
    // later if needed.
}

#[derive(Debug, Clone, Component)]
pub struct Collider {
    pub size: Vec2,
    pub collides_with_player: bool,
    pub collides_with_projectile: bool,
}

// fn to check if two entities are colliding
pub fn check_collision(a: &Vec3, a_collider: &Collider, b: &Vec3, b_collider: &Collider) -> bool {
    let a_min = a.truncate() - a_collider.size / 2.0;
    let a_max = a.truncate() + a_collider.size / 2.0;
    let b_min = b.truncate() - b_collider.size / 2.0;
    let b_max = b.truncate() + b_collider.size / 2.0;

    a_min.x < b_max.x && a_max.x > b_min.x && a_min.y < b_max.y && a_max.y > b_min.y
}
