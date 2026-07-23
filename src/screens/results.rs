//! Post-match leaderboard screen.

use bevy::prelude::*;

use crate::{
    demo::lib::{player_color_label, player_color_name, MatchResults, PendingSessionTeardown},
    screens::Screen,
    theme::{
        interaction::{InteractionPalette, OnPress},
        palette::*,
        prelude::*,
    },
};

pub(super) fn plugin(app: &mut App) {
    app.add_systems(OnEnter(Screen::Results), spawn_results_screen);
}

fn spawn_results_screen(mut commands: Commands, results: Option<Res<MatchResults>>) {
    let rankings = results.map(|r| r.rankings.clone()).unwrap_or_default();

    commands
        .ui_root()
        .insert(StateScoped(Screen::Results))
        .with_children(|children| {
            children.spawn((
                Name::new("Results Header"),
                Text::new("Match Over"),
                TextFont::from_font_size(40.0),
                TextColor(HEADER_TEXT),
            ));

            if rankings.is_empty() {
                children.spawn((
                    Name::new("Empty Leaderboard"),
                    Text::new("No scores recorded"),
                    TextFont::from_font_size(24.0),
                    TextColor(LABEL_TEXT),
                ));
            } else {
                for (place, entry) in rankings.iter().enumerate() {
                    children.spawn((
                        Name::new("Leaderboard Row"),
                        Text::new(format!(
                            "{}. {} — {} coins",
                            place + 1,
                            player_color_name(entry.color),
                            entry.score
                        )),
                        TextFont::from_font_size(26.0),
                        TextColor(player_color_label(entry.color)),
                    ));
                }
            }

            children
                .spawn((
                    Name::new("Return To Menu Button"),
                    Button,
                    Node {
                        width: Val::Px(240.0),
                        height: Val::Px(48.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        margin: UiRect::top(Val::Px(24.0)),
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
                        Name::new("Return To Menu Label"),
                        Text::new("Main Menu"),
                        TextFont::from_font_size(22.0),
                        TextColor(BUTTON_TEXT),
                    ));
                })
                .observe(return_to_main_menu);
        });
}

fn return_to_main_menu(
    _trigger: Trigger<OnPress>,
    mut commands: Commands,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    bevy::log::info!("Returning to main menu from results");
    commands.remove_resource::<MatchResults>();
    next_screen.set(Screen::Title);
    commands.insert_resource(PendingSessionTeardown);
}
