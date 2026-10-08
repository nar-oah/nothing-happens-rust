use bevy::{ecs::schedule::common_conditions::any_match_filter, prelude::*};

use crate::metrics::{
    AnnualInfluence, ImmediateInfluence, MONTH_METRIC_ID, Metric, MetricChange, MetricId,
    MetricMetadata, MetricOrder, MetricValue, PendingMetricChanges, PendingMonthAdvances,
    SimulationSet, TERM_METRIC_ID, YEAR_METRIC_ID,
};

pub struct DebugUiPlugin;

impl Plugin for DebugUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            setup_ui
                .run_if(any_match_filter::<Added<Metric>>.and_then(run_once))
                .after(SimulationSet::AdvanceTime)
                .before(refresh_labels),
        )
            .add_systems(Update, handle_buttons.before(SimulationSet::ApplyChanges))
            .add_systems(Update, refresh_labels.after(SimulationSet::AdvanceTime));
    }
}

#[derive(Component)]
struct ChangeMetricButton(MetricChange);

#[derive(Component)]
struct NextMonthButton;

#[derive(Component)]
enum DisplayValue {
    Date,
    Metric(Entity),
}

const NORMAL_BUTTON: Color = Color::srgb(0.18, 0.23, 0.30);
const HOVERED_BUTTON: Color = Color::srgb(0.26, 0.34, 0.43);
const PRESSED_BUTTON: Color = Color::srgb(0.19, 0.45, 0.38);
const PRIMARY_TEXT: Color = Color::srgb(0.92, 0.94, 0.97);
const SECONDARY_TEXT: Color = Color::srgb(0.65, 0.71, 0.79);

fn label(text: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont::from(FontSource::SansSerif).with_font_size(FontSize::Px(size)),
        TextColor(color),
    )
}

fn button(text: impl Into<String>) -> impl Bundle {
    (
        Button,
        Node {
            min_width: px(74),
            height: px(36),
            padding: UiRect::horizontal(px(12)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border_radius: BorderRadius::all(px(4)),
            ..default()
        },
        BackgroundColor(NORMAL_BUTTON),
        children![label(text, 16.0, PRIMARY_TEXT)],
    )
}

type MetricRowData = (
    Entity,
    &'static MetricId,
    &'static MetricMetadata,
    &'static MetricValue,
    &'static MetricOrder,
    Has<ImmediateInfluence>,
    Has<AnnualInfluence>,
);

type MetricLabelData = (
    &'static MetricId,
    &'static MetricMetadata,
    &'static MetricValue,
    Has<ImmediateInfluence>,
    Has<AnnualInfluence>,
);

fn setup_ui(mut commands: Commands, metrics: Query<MetricRowData, With<Metric>>) {
    let mut rows: Vec<_> = metrics.iter().collect();
    rows.sort_by_key(|(_, _, _, _, order, _, _)| order.0);
    commands.spawn(Camera2d);
    commands
        .spawn(Node {
            width: percent(100),
            height: percent(100),
            padding: UiRect::all(px(20)),
            flex_direction: FlexDirection::Column,
            row_gap: px(10),
            ..default()
        })
        .with_children(|root| {
            root.spawn(label("NOTHING HAPPENS · 指标调试", 26.0, PRIMARY_TEXT));
            root.spawn((
                label("", 22.0, Color::srgb(0.42, 0.83, 0.69)),
                DisplayValue::Date,
            ));
            root.spawn(label(
                "每次推进一个月；12 月结束后进行 Annual 结算。",
                16.0,
                SECONDARY_TEXT,
            ));

            for &(entity, id, metadata, value, _, immediate, annual) in &rows {
                root.spawn((
                    Node {
                        width: percent(100),
                        padding: UiRect::all(px(10)),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4),
                        border_radius: BorderRadius::all(px(5)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.10, 0.12, 0.16)),
                ))
                .with_children(|row| {
                    row.spawn(Node {
                        width: percent(100),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::SpaceBetween,
                        ..default()
                    })
                    .with_children(|header| {
                        header.spawn((
                            label(
                                metric_text(metadata, value, immediate, annual),
                                20.0,
                                PRIMARY_TEXT,
                            ),
                            DisplayValue::Metric(entity),
                        ));
                        header
                            .spawn(Node {
                                column_gap: px(8),
                                ..default()
                            })
                            .with_children(|controls| {
                                let changes: &[f64] = if id.0 == "collapse" {
                                    &[25.0, 100.0]
                                } else if matches!(
                                    id.0.as_str(),
                                    YEAR_METRIC_ID | MONTH_METRIC_ID | TERM_METRIC_ID
                                ) {
                                    &[-1.0, 1.0]
                                } else {
                                    &[-5.0, 5.0]
                                };
                                for &delta in changes {
                                    controls.spawn((
                                        button(format!("{delta:+.0}")),
                                        ChangeMetricButton(MetricChange {
                                            target: entity,
                                            delta,
                                        }),
                                    ));
                                }
                            });
                    });
                    row.spawn(label(&metadata.description, 14.0, SECONDARY_TEXT));
                });
            }

            root.spawn((button("Next Month"), NextMonthButton));
            root.spawn(label("Esc：退出", 13.0, SECONDARY_TEXT));
        });
}

fn metric_text(
    metadata: &MetricMetadata,
    value: &MetricValue,
    immediate: bool,
    annual: bool,
) -> String {
    let kind = if annual {
        "Annual"
    } else if immediate {
        "Immediate"
    } else {
        "Manual"
    };
    format!("{}  {:.2}  ·  {kind}", metadata.name, value.0)
}

fn handle_buttons(
    mut buttons: Query<
        (
            &Interaction,
            Option<&ChangeMetricButton>,
            Has<NextMonthButton>,
            &mut BackgroundColor,
        ),
        (With<Button>, Changed<Interaction>),
    >,
    mut changes: ResMut<PendingMetricChanges>,
    mut months: ResMut<PendingMonthAdvances>,
) {
    for (interaction, change, next_month, mut color) in &mut buttons {
        *color = match interaction {
            Interaction::Pressed => {
                if let Some(change) = change {
                    changes.0.push_back(change.0);
                }
                if next_month {
                    months.0 += 1;
                }
                PRESSED_BUTTON
            }
            Interaction::Hovered => HOVERED_BUTTON,
            Interaction::None => NORMAL_BUTTON,
        }
        .into();
    }
}

fn refresh_labels(
    metrics: Query<MetricLabelData, With<Metric>>,
    mut labels: Query<(&DisplayValue, &mut Text)>,
) {
    for (display, mut text) in &mut labels {
        let value = match display {
            DisplayValue::Date => {
                let value = |id| {
                    metrics
                        .iter()
                        .find_map(|(metric_id, _, value, _, _)| {
                            (metric_id.0 == id).then_some(value.0)
                        })
                        .expect("time and term metrics exist")
                };
                format!(
                    "Year {} / Month {} / Term {}",
                    value(YEAR_METRIC_ID),
                    value(MONTH_METRIC_ID),
                    value(TERM_METRIC_ID)
                )
            }
            DisplayValue::Metric(entity) => {
                let (_, metadata, value, immediate, annual) =
                    metrics.get(*entity).expect("metric entity exists");
                metric_text(metadata, value, immediate, annual)
            }
        };
        if text.0 != value {
            text.0 = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use bevy::asset::AssetPlugin;

    use super::*;
    use crate::metrics::MetricPlugin;

    fn press(app: &mut App, entity: Entity) {
        let world = app.world_mut();
        *world.get_mut::<Interaction>(entity).unwrap() = Interaction::None;
        app.update();
        *app.world_mut().get_mut::<Interaction>(entity).unwrap() = Interaction::Pressed;
        app.update();
    }

    fn change_button(app: &mut App, id: &str, delta: f64) -> Entity {
        let world = app.world_mut();
        let target = world
            .query::<(Entity, &MetricId)>()
            .iter(world)
            .find_map(|(entity, metric_id)| (metric_id.0 == id).then_some(entity))
            .unwrap();
        world
            .query::<(Entity, &ChangeMetricButton)>()
            .iter(world)
            .find_map(|(entity, button)| {
                (button.0 == MetricChange { target, delta }).then_some(entity)
            })
            .expect("metric change has a UI button")
    }

    fn displays(app: &mut App, expected: &str) -> bool {
        let world = app.world_mut();
        world
            .query::<&Text>()
            .iter(world)
            .any(|text| text.0 == expected)
    }

    #[test]
    fn buttons_only_submit_requests_and_held_buttons_do_not_repeat() {
        let mut app = App::new();
        app.init_resource::<PendingMetricChanges>()
            .init_resource::<PendingMonthAdvances>()
            .add_systems(Update, handle_buttons);
        let target = app.world_mut().spawn((Metric, MetricValue(10.0))).id();
        let change = MetricChange { target, delta: 5.0 };
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            BackgroundColor(NORMAL_BUTTON),
            ChangeMetricButton(change),
        ));
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            BackgroundColor(NORMAL_BUTTON),
            NextMonthButton,
        ));

        app.update();

        assert_eq!(app.world().get::<MetricValue>(target).unwrap().0, 10.0);
        assert_eq!(
            app.world().resource::<PendingMetricChanges>().0,
            std::collections::VecDeque::from([change]),
        );
        assert_eq!(app.world().resource::<PendingMonthAdvances>().0, 1);
        app.update();
        assert_eq!(app.world().resource::<PendingMetricChanges>().0.len(), 1);
        assert_eq!(app.world().resource::<PendingMonthAdvances>().0, 1);
    }

    #[test]
    fn ui_is_created_once_when_metric_entities_are_spawned() {
        let mut app = App::new();
        app.init_resource::<PendingMetricChanges>()
            .init_resource::<PendingMonthAdvances>()
            .add_plugins(DebugUiPlugin);

        for _ in 0..2 {
            app.update();
            let world = app.world_mut();
            assert_eq!(world.query::<&Camera2d>().iter(world).count(), 0);
            assert_eq!(world.query::<&Button>().iter(world).count(), 0);
        }

        app.add_systems(
            Update,
            (|mut commands: Commands| {
                for (order, id) in [YEAR_METRIC_ID, MONTH_METRIC_ID, TERM_METRIC_ID]
                    .into_iter()
                    .enumerate()
                {
                    commands.spawn((
                        Metric,
                        MetricId(id.into()),
                        MetricMetadata {
                            name: id.into(),
                            description: id.into(),
                        },
                        MetricValue(1.0),
                        MetricOrder(order),
                    ));
                }
            })
            .run_if(run_once)
            .before(SimulationSet::AdvanceTime),
        );

        for _ in 0..3 {
            app.update();
            assert!(displays(&mut app, "Year 1 / Month 1 / Term 1"));
            let world = app.world_mut();
            assert_eq!(world.query::<&Camera2d>().iter(world).count(), 1);
            assert_eq!(world.query::<&Button>().iter(world).count(), 7);
        }
    }

    #[test]
    fn buttons_run_the_loop_and_refresh_labels_in_the_same_frame() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(AssetPlugin {
                file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets").into(),
                ..default()
            })
            .add_plugins((MetricPlugin, DebugUiPlugin));
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            app.update();
            if displays(&mut app, "Year 1 / Month 1 / Term 1") {
                break;
            }
            assert!(Instant::now() < deadline, "metric UI did not load in time");
            std::thread::yield_now();
        }

        let productivity = change_button(&mut app, "productivity", 5.0);
        press(&mut app, productivity);
        assert!(displays(&mut app, "生产力  15.00  ·  Manual"));
        assert!(displays(&mut app, "产出  30.00  ·  Immediate"));
        // A held button must not apply the change again in the next frame.
        app.update();
        assert!(displays(&mut app, "生产力  15.00  ·  Manual"));

        let world = app.world_mut();
        let next_month = world
            .query_filtered::<Entity, With<NextMonthButton>>()
            .single(world)
            .unwrap();
        for _ in 0..12 {
            press(&mut app, next_month);
        }
        assert!(displays(&mut app, "Year 2 / Month 1 / Term 1"));
        assert!(displays(&mut app, "年收入  45.00  ·  Annual"));
        assert!(displays(&mut app, "储备  72.50  ·  Immediate"));

        let collapse = change_button(&mut app, "collapse", 100.0);
        press(&mut app, collapse);
        assert!(displays(&mut app, "Year 1 / Month 1 / Term 2"));
        assert!(displays(&mut app, "生产力  10.00  ·  Manual"));
        assert!(displays(&mut app, "年收入  0.00  ·  Annual"));
        assert!(displays(&mut app, "储备  50.00  ·  Immediate"));
        assert!(displays(&mut app, "崩溃度  0.00  ·  Manual"));
        assert!(displays(&mut app, "任期  2.00  ·  Manual"));

        let year = change_button(&mut app, YEAR_METRIC_ID, 1.0);
        let month = change_button(&mut app, MONTH_METRIC_ID, 1.0);
        let term = change_button(&mut app, TERM_METRIC_ID, 1.0);
        press(&mut app, year);
        press(&mut app, month);
        press(&mut app, term);
        assert!(displays(&mut app, "Year 2 / Month 2 / Term 3"));
        assert!(displays(&mut app, "年份  2.00  ·  Manual"));
        assert!(displays(&mut app, "月份  2.00  ·  Manual"));
    }
}
