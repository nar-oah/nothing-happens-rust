use bevy::prelude::*;

use crate::metrics::{
    AnnualInfluence, GameAction, GameDate, GameStatus, ImmediateInfluence, Metric, MetricId,
    MetricMetadata, MetricOrder, MetricValue, PendingActions, SimulationSet,
};

pub struct DebugUiPlugin;

impl Plugin for DebugUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, setup_ui)
            .add_systems(Update, handle_buttons.before(SimulationSet::ApplyActions))
            .add_systems(Update, refresh_labels.after(SimulationSet::ApplyActions));
    }
}

#[derive(Component)]
struct DebugAction(GameAction);

#[derive(Component)]
enum DisplayValue {
    Date,
    Status,
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

fn button(text: impl Into<String>, action: GameAction) -> impl Bundle {
    (
        Button,
        DebugAction(action),
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
            padding: UiRect::all(px(28)),
            flex_direction: FlexDirection::Column,
            row_gap: px(14),
            ..default()
        })
        .with_children(|root| {
            root.spawn(label("NOTHING HAPPENS · 指标调试", 26.0, PRIMARY_TEXT));
            root.spawn((
                label("Year 1 / Month 1", 22.0, Color::srgb(0.42, 0.83, 0.69)),
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
                        padding: UiRect::all(px(14)),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(6),
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
                            label(metric_text(metadata, value, immediate, annual), 20.0, PRIMARY_TEXT),
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
                                } else {
                                    &[-5.0, 5.0]
                                };
                                for &delta in changes {
                                    controls.spawn(button(
                                        format!("{delta:+.0}"),
                                        GameAction::ChangeMetric {
                                            id: id.0.clone(),
                                            delta,
                                        },
                                    ));
                                }
                            });
                    });
                    row.spawn(label(&metadata.description, 14.0, SECONDARY_TEXT));
                });
            }

            root.spawn(button("Next Month", GameAction::NextMonth));
            root.spawn((label("", 15.0, SECONDARY_TEXT), DisplayValue::Status));
            root.spawn(label("Esc：退出", 13.0, SECONDARY_TEXT));
        });
}

fn metric_text(metadata: &MetricMetadata, value: &MetricValue, immediate: bool, annual: bool) -> String {
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
    mut buttons: Query<(&Interaction, &DebugAction, &mut BackgroundColor), Changed<Interaction>>,
    mut actions: ResMut<PendingActions>,
) {
    for (interaction, action, mut color) in &mut buttons {
        *color = match interaction {
            Interaction::Pressed => {
                actions.0.push_back(action.0.clone());
                PRESSED_BUTTON
            }
            Interaction::Hovered => HOVERED_BUTTON,
            Interaction::None => NORMAL_BUTTON,
        }
        .into();
    }
}

fn refresh_labels(
    date: Res<GameDate>,
    status: Res<GameStatus>,
    metrics: Query<MetricLabelData, With<Metric>>,
    mut labels: Query<(&DisplayValue, &mut Text)>,
) {
    for (display, mut text) in &mut labels {
        let value = match display {
            DisplayValue::Date => format!("Year {} / Month {}", date.year, date.month),
            DisplayValue::Status => {
                format!("重开次数：{}  ·  {}", status.restarts, status.last_action)
            }
            DisplayValue::Metric(entity) => {
                let (metadata, value, immediate, annual) = metrics.get(*entity).expect("metric entity exists");
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
    use super::*;
    use crate::metrics::MetricPlugin;

    fn press(app: &mut App, action: GameAction) {
        let world = app.world_mut();
        let entity = world
            .query::<(Entity, &DebugAction)>()
            .iter(world)
            .find_map(|(entity, button)| (button.0 == action).then_some(entity))
            .expect("action has a UI button");
        *world.get_mut::<Interaction>(entity).unwrap() = Interaction::None;
        app.update();
        *app.world_mut().get_mut::<Interaction>(entity).unwrap() = Interaction::Pressed;
        app.update();
    }

    fn displays(app: &mut App, expected: &str) -> bool {
        let world = app.world_mut();
        world
            .query::<&Text>()
            .iter(world)
            .any(|text| text.0 == expected)
    }

    #[test]
    fn buttons_run_the_loop_and_refresh_labels_in_the_same_frame() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_plugins((
            MetricPlugin::from_ron(include_str!("../assets/metrics.ron")).unwrap(),
            DebugUiPlugin,
        ));
        app.update();
        assert!(displays(&mut app, "Year 1 / Month 1"));

        press(
            &mut app,
            GameAction::ChangeMetric {
                id: "productivity".into(),
                delta: 5.0,
            },
        );
        assert!(displays(&mut app, "生产力  15.00  ·  Manual"));
        assert!(displays(&mut app, "产出  30.00  ·  Immediate"));
        // A held button must not apply the change again in the next frame.
        app.update();
        assert!(displays(&mut app, "生产力  15.00  ·  Manual"));

        for _ in 0..12 {
            press(&mut app, GameAction::NextMonth);
        }
        assert!(displays(&mut app, "Year 2 / Month 1"));
        assert!(displays(&mut app, "年收入  45.00  ·  Annual"));
        assert!(displays(&mut app, "储备  72.50  ·  Immediate"));

        press(
            &mut app,
            GameAction::ChangeMetric {
                id: "collapse".into(),
                delta: 100.0,
            },
        );
        assert!(displays(&mut app, "Year 1 / Month 1"));
        assert!(displays(&mut app, "生产力  10.00  ·  Manual"));
        assert!(displays(&mut app, "年收入  0.00  ·  Annual"));
        assert!(displays(&mut app, "储备  50.00  ·  Immediate"));
        assert!(displays(&mut app, "崩溃度  0.00  ·  Manual"));
        assert_eq!(app.world().resource::<GameStatus>().restarts, 1);
    }
}
