use bevy::{app::AppExit, prelude::*};
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

    app.run();
    Ok(())
}

fn exit_on_escape(keys: Res<ButtonInput<KeyCode>>, mut exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}
