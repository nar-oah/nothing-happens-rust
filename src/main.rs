use bevy::{
    app::AppExit,
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};
use nothing_happens::{debug_ui::DebugUiPlugin, metrics::MetricPlugin};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let packaged_metrics = std::env::current_exe()?
        .with_file_name("assets")
        .join("metrics.ron");
    let metrics = MetricPlugin::from_path(if packaged_metrics.is_file() {
        packaged_metrics
    } else {
        concat!(env!("CARGO_MANIFEST_DIR"), "/assets/metrics.ron").into()
    })?;
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

fn finish_smoke_test(
    mut commands: Commands,
    mut frames: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    *frames += 1;
    if *frames == 60 {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(
                std::env::temp_dir().join("nothing-happens-debug-ui.png"),
            ));
    }
    if *frames == 120 {
        info!("Runtime smoke check complete: UI ran for 120 frames.");
        exit.write(AppExit::Success);
    }
}
