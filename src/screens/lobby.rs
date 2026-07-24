//! Lobby screen: ready indicators, lobby status text, and session controls.

use bevy::prelude::*;

use crate::{
    demo::{
        client::{ClientLobby, ControlledPlayer},
        lib::{Player, PlayerCommand},
    },
    screens::{gameplay::ScoreText, Screen},
    theme::{
        interaction::{InteractionPalette, OnPress},
        palette::*,
    },
};

pub(super) fn plugin(app: &mut App) {
    app.add_event::<ToggleReadyEvent>();
    app.add_systems(OnEnter(Screen::Lobby), spawn_lobby_ui);
    app.add_systems(OnEnter(Screen::Title), despawn_session_hud);
    app.add_systems(OnEnter(Screen::Results), despawn_session_hud);
    app.add_systems(
        Update,
        (
            add_ready_checker,
            update_ready_checker,
            update_lobby_details,
        )
            .run_if(in_state(Screen::Lobby)),
    );
    app.add_systems(OnExit(Screen::Lobby), despawn_ready_checker);
}

const NOT_READY_COLOR: Color = Color::srgb(0.9, 0.1, 0.1);
const READY_COLOR: Color = Color::srgb(0.1, 0.9, 0.1);

#[derive(Component)]
pub struct ReadyTracker;

#[derive(Component)]
struct SessionHud;

#[derive(Component)]
struct LobbyDetailsText;

#[derive(Component)]
struct ResetServerButton;

#[derive(Debug, Event)]
pub struct ToggleReadyEvent {
    pub player: Entity,
    pub is_ready: bool,
}

fn spawn_lobby_ui(mut commands: Commands, existing: Query<Entity, With<SessionHud>>) {
    if !existing.is_empty() {
        return;
    }

    // Left stack: coins + reset (persists into gameplay until title).
    commands
        .spawn((
            Name::new("Session HUD"),
            SessionHud,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(10.0),
                top: Val::Px(10.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                align_items: AlignItems::FlexStart,
                ..default()
            },
        ))
        .with_children(|parent| {
            parent
                .spawn((
                    Name::new("Coins Label"),
                    Text::new("Coins: 0"),
                    TextFont::from_font_size(24.0),
                    TextColor(LOBBY_TEXT),
                    ScoreText,
                ));

            parent
                .spawn((
                    Name::new("Reset Server Button"),
                    Button,
                    ResetServerButton,
                    Node {
                        width: Val::Px(180.0),
                        height: Val::Px(40.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(NODE_BACKGROUND),
                    InteractionPalette {
                        none: NODE_BACKGROUND,
                        hovered: BUTTON_HOVERED_BACKGROUND,
                        pressed: BUTTON_PRESSED_BACKGROUND,
                    },
                ))
                .with_children(|btn| {
                    btn.spawn((
                        Name::new("Reset Server Label"),
                        Text::new("Reset Server"),
                        TextFont::from_font_size(20.0),
                        TextColor(BUTTON_TEXT),
                    ));
                })
                .observe(request_server_reset);
        });

    // Center/top lobby details (lobby only).
    commands
        .spawn((
            Name::new("Lobby Details Root"),
            StateScoped(Screen::Lobby),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::FlexStart,
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                padding: UiRect::top(Val::Px(24.0)),
                position_type: PositionType::Absolute,
                ..default()
            },
        ))
        .with_children(|children| {
            children.spawn((
                Name::new("Lobby Details"),
                LobbyDetailsText,
                Text::new("Lobby\nConnecting…"),
                TextFont::from_font_size(22.0),
                TextColor(LOBBY_TEXT),
                TextLayout::new_with_justify(JustifyText::Center),
                Node {
                    width: Val::Px(480.0),
                    ..default()
                },
            ));
        });
}

fn despawn_session_hud(mut commands: Commands, hud: Query<Entity, With<SessionHud>>) {
    for entity in &hud {
        commands.entity(entity).despawn_recursive();
    }
}

fn request_server_reset(
    _trigger: Trigger<OnPress>,
    mut player_commands: EventWriter<PlayerCommand>,
) {
    bevy::log::info!("Reset Server pressed");
    player_commands.send(PlayerCommand::ResetServer);
}

fn update_lobby_details(
    mut details: Query<&mut Text, With<LobbyDetailsText>>,
    lobby: Res<ClientLobby>,
    players: Query<&Player>,
    local_player: Query<&Player, With<ControlledPlayer>>,
) {
    let Ok(mut text) = details.get_single_mut() else {
        return;
    };

    let total = lobby.players.len();
    let mut ready_count = 0usize;
    for info in lobby.players.values() {
        if let Ok(player) = players.get(info.client_entity) {
            if player.is_ready {
                ready_count += 1;
            }
        }
    }

    let you = local_player
        .get_single()
        .map(|p| if p.is_ready { "ready" } else { "not ready" })
        .unwrap_or("connecting…");

    let start_hint = if total < 2 {
        "Need at least 2 players to start"
    } else if ready_count < total {
        "Waiting for everyone to ready up"
    } else {
        "Starting…"
    };

    text.0 = format!(
        "Lobby\nPlayers: {total}\nReady: {ready_count}/{total}\nYou: {you}\n\n[Space] toggle ready\n{start_hint}"
    );
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
    tracker_query: Query<(Entity, &Parent), With<ReadyTracker>>,
    mut commands: Commands,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for player in toggles.read() {
        for (e, parent) in tracker_query.iter() {
            if parent.get() == player.player {
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
    for ready_entity in ready_query.iter() {
        commands.entity(ready_entity).despawn_recursive();
    }
}
