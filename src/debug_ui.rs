use bevy::prelude::*;

use crate::metrics::{
    GameAction, GameDate, GameStatus, Influence, Metric, MetricEntities, PendingActions, SimulationSet,
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

fn setup_ui(mut commands: Commands, entities: Res<MetricEntities>, metrics: Query<&Metric>) {
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

            for &entity in &entities.ordered {
                let metric = metrics.get(entity).expect("metric entity exists");
                let definition = &metric.definition;
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
                            label(metric_text(metric), 20.0, PRIMARY_TEXT),
                            DisplayValue::Metric(entity),
                        ));
                        header
                            .spawn(Node {
                                column_gap: px(8),
                                ..default()
                            })
                            .with_children(|controls| {
                                let changes: &[f64] = if definition.id == "collapse" {
                                    &[25.0, 100.0]
                                } else {
                                    &[-5.0, 5.0]
                                };
                                for &delta in changes {
                                    controls.spawn(button(
                                        format!("{delta:+.0}"),
                                        GameAction::ChangeMetric {
                                            id: definition.id.clone(),
                                            delta,
                                        },
                                    ));
                                }
                            });
                    });
                    row.spawn(label(&definition.description, 14.0, SECONDARY_TEXT));
                });
            }

            root.spawn(button("Next Month", GameAction::NextMonth));
            root.spawn((label("", 15.0, SECONDARY_TEXT), DisplayValue::Status));
            root.spawn(label("Esc：退出", 13.0, SECONDARY_TEXT));
        });
}

fn metric_text(metric: &Metric) -> String {
    let kind = match metric.definition.influence {
        Some(Influence::Immediate(_)) => "Immediate",
        Some(Influence::Annual(_)) => "Annual",
        None => "Manual",
    };
    format!("{}  {:.2}  ·  {kind}", metric.definition.name, metric.value)
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
    metrics: Query<&Metric>,
    mut labels: Query<(&DisplayValue, &mut Text)>,
) {
    for (display, mut text) in &mut labels {
        let value = match display {
            DisplayValue::Date => format!("Year {} / Month {}", date.year, date.month),
            DisplayValue::Status => {
                format!("重开次数：{}  ·  {}", status.restarts, status.last_action)
            }
            DisplayValue::Metric(entity) => {
                metric_text(metrics.get(*entity).expect("metric entity exists"))
            }
        };
        if text.0 != value {
            text.0 = value;
        }
    }
}
