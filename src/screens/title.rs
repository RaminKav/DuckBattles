//! The title screen that appears when the game starts.

use bevy::prelude::*;
use bevy_mod_reqwest::*;

use crate::{
    demo::{
        client::ClientNetworkConfig,
        lib::{PublicServerStatus, ServerPhase},
    },
    screens::Screen,
    theme::prelude::*,
};

pub(super) fn plugin(app: &mut App) {
    app.insert_resource(ServerStatusPoll {
        timer: Timer::from_seconds(2.0, TimerMode::Repeating),
    });
    app.add_systems(OnEnter(Screen::Title), spawn_title_screen);
    app.add_systems(
        Update,
        poll_server_status.run_if(in_state(Screen::Title)),
    );
}

#[derive(Component)]
struct ServerStatusText;

#[derive(Resource)]
struct ServerStatusPoll {
    timer: Timer,
}

fn spawn_title_screen(mut commands: Commands, mut poll: ResMut<ServerStatusPoll>) {
    // Fire immediately on enter.
    let duration = poll.timer.duration();
    poll.timer.set_elapsed(duration);

    commands
        .ui_root()
        .insert(StateScoped(Screen::Title))
        .with_children(|children| {
            children.button("Play").observe(enter_lobby_screen);

            children.spawn((
                Name::new("Server Status"),
                ServerStatusText,
                Text::new("Checking server…"),
                TextFont::from_font_size(22.0),
                TextColor(ui_palette::LOBBY_TEXT),
                TextLayout::new_with_justify(JustifyText::Center),
            ));

            #[cfg(not(target_family = "wasm"))]
            children.button("Exit").observe(exit_app);
        });
}

fn poll_server_status(
    time: Res<Time>,
    mut poll: ResMut<ServerStatusPoll>,
    mut client: BevyReqwest,
    config: Res<ClientNetworkConfig>,
) {
    if !poll.timer.tick(time.delta()).just_finished() {
        return;
    }

    let url = config.status_endpoint();
    let Ok(reqwest_request) = client.get(&url).build() else {
        bevy::log::warn!("Failed to build status request for {url}");
        return;
    };

    client
        .send(reqwest_request)
        .on_json_response(
            |trigger: Trigger<JsonResponse<PublicServerStatus>>,
             mut status_text: Query<&mut Text, With<ServerStatusText>>| {
                let status = &trigger.event().0;
                let Ok(mut text) = status_text.get_single_mut() else {
                    return;
                };
                text.0 = format_server_status(status);
            },
        )
        .on_error(
            |trigger: Trigger<ReqwestErrorEvent>,
             mut status_text: Query<&mut Text, With<ServerStatusText>>| {
                let e = &trigger.event().0;
                bevy::log::debug!("Server status poll failed: {e:?}");
                let Ok(mut text) = status_text.get_single_mut() else {
                    return;
                };
                text.0 = "Server offline".to_string();
            },
        );
}

fn format_server_status(status: &PublicServerStatus) -> String {
    match status.phase {
        ServerPhase::Lobby => {
            format!("Lobby: {}/{}", status.players, status.max_players)
        }
        ServerPhase::Match => {
            let secs = status.remaining_secs.unwrap_or(0);
            format!("Match in progress: {secs}s left")
        }
    }
}

fn enter_lobby_screen(_trigger: Trigger<OnPress>, mut next_screen: ResMut<NextState<Screen>>) {
    next_screen.set(Screen::Lobby);
}

#[cfg(not(target_family = "wasm"))]
fn exit_app(_trigger: Trigger<OnPress>, mut app_exit: EventWriter<AppExit>) {
    app_exit.send(AppExit::Success);
}
