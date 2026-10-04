use bevy::{app::AppExit, prelude::*};
use nothing_happens::{debug_ui::DebugUiPlugin, metrics::MetricPlugin};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Nothing Happens — Phase 1".into(),
                resolution: (960, 1000).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.06, 0.07, 0.09)))
        .add_plugins((MetricPlugin, DebugUiPlugin))
        .add_systems(Update, exit_on_escape)
        .run();
}

fn exit_on_escape(keys: Res<ButtonInput<KeyCode>>, mut exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}
