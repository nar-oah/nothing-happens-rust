use bevy::{app::AppExit, prelude::*};
use nothing_happens::{debug_ui::DebugUiPlugin, metrics::MetricPlugin};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let metrics =
        MetricPlugin::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/metrics.ron"))?;
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Nothing Happens — Phase 1".into(),
            resolution: (960, 780).into(),
            ..default()
        }),
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgb(0.06, 0.07, 0.09)))
    .add_plugins((metrics, DebugUiPlugin))
    .add_systems(Update, exit_on_escape);

    // Open the real UI, render a few frames, and exit cleanly for a runtime smoke check.
    if std::env::args().any(|argument| argument == "--smoke-test") {
        app.add_systems(Update, finish_smoke_test);
    }
    app.run();
    Ok(())
}

fn exit_on_escape(keys: Res<ButtonInput<KeyCode>>, mut exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}

fn finish_smoke_test(mut frames: Local<u32>, mut exit: MessageWriter<AppExit>) {
    *frames += 1;
    if *frames == 120 {
        info!("Runtime smoke check complete: UI ran for 120 frames.");
        exit.write(AppExit::Success);
    }
}
