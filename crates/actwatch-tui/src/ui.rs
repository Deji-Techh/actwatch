use chrono::{Datelike, Local};
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Paragraph, Row, Table, TableState, Widget,
    },
    Frame,
};

use crate::app::{format_duration, format_pct, format_rss, App, AppMode, SortMode, SortOrder};
use crate::db::Category;
use crate::theme;

// Strict monochrome palette per docs/ARCHITECTURE.md:
// bg #0B0B0C, surface #141416, text #F4F4F5, muted #8E8E93, hairlines #262628. Zero hue.
const THEME_BG: Color = Color::Rgb(11, 11, 12);          // #0B0B0C
const THEME_SURFACE: Color = Color::Rgb(20, 20, 22);      // #141416
const THEME_HAIRLINE: Color = Color::Rgb(38, 38, 40);     // #262628
const THEME_TEXT: Color = Color::Rgb(244, 244, 245);      // #F4F4F5
const THEME_MUTED: Color = Color::Rgb(142, 142, 147);     // #8E8E93
const THEME_ROW_ALT: Color = Color::Rgb(24, 24, 27);      // #18181B
const THEME_SELECTED: Color = Color::Rgb(44, 44, 48);     // #2C2C30
const THEME_WHITE: Color = Color::Rgb(255, 255, 255);     // #FFFFFF

pub fn render_ui(f: &mut Frame, app: &mut App) {
    let size = f.area();

    // If terminal is too small, render warning
    if size.width < 50 || size.height < 15 {
        let msg = Paragraph::new("Terminal too small for actwatch-tui\nPlease resize window.")
            .style(Style::default().fg(THEME_MUTED))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(THEME_HAIRLINE))
                    .style(Style::default().bg(THEME_BG)),
            );
        f.render_widget(msg, size);
        return;
    }

    if app.data.is_empty_db {
        render_empty_db(f, app, size);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),  // Header
            Constraint::Length(14), // Charts & Categories + Away card
            Constraint::Min(8),     // App Table
            Constraint::Length(1),  // Footer / Keybindings
        ])
        .split(size);

    render_header(f, app, chunks[0]);
    render_middle_section(f, app, chunks[1]);
    render_app_table(f, app, chunks[2]);
    render_footer(f, app, chunks[3]);
}

fn render_empty_db(f: &mut Frame, app: &App, area: Rect) {
    let vertical_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Length(13),
            Constraint::Min(2),
        ])
        .split(area);

    let center_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(60),
            Constraint::Percentage(20),
        ])
        .split(vertical_chunks[1]);

    let card_area = center_chunks[1];

    let block = Block::default()
        .title(" actwatch — Terminal Activity Monitor ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(THEME_HAIRLINE))
        .style(Style::default().bg(THEME_SURFACE));

    let text = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  ▪  ", Style::default().fg(THEME_MUTED)),
            Span::styled(
                "No activity data recorded yet",
                Style::default().fg(THEME_TEXT).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Database: ", Style::default().fg(THEME_MUTED)),
            Span::styled(
                app.db.path().to_string_lossy(),
                Style::default().fg(THEME_TEXT),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Status:   ", Style::default().fg(THEME_MUTED)),
            Span::styled(
                "Waiting for actwatch-daemon to record activity...",
                Style::default().fg(THEME_MUTED),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [r] ", Style::default().fg(THEME_WHITE).add_modifier(Modifier::BOLD)),
            Span::styled("Refresh now    ", Style::default().fg(THEME_MUTED)),
            Span::styled("  [q] ", Style::default().fg(THEME_WHITE).add_modifier(Modifier::BOLD)),
            Span::styled("Quit", Style::default().fg(THEME_MUTED)),
        ]),
    ];

    let paragraph = Paragraph::new(text).block(block).alignment(Alignment::Left);
    f.render_widget(paragraph, card_area);

    // Minimal footer
    render_footer(f, app, Rect {
        x: area.x,
        y: area.height.saturating_sub(1),
        width: area.width,
        height: 1,
    });
}

fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let header_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(THEME_HAIRLINE))
        .style(Style::default().bg(THEME_SURFACE));

    let inner = header_block.inner(area);
    f.render_widget(header_block, area);

    let header_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(60),
            Constraint::Percentage(40),
        ])
        .split(inner);

    // Left: Day title + Daily Usage + WoW Delta
    let is_today = app.selected_date == Local::now().date_naive();
    let today_tag = if is_today { " (Today)" } else { "" };
    let date_str = format!(
        "◀  {} {}{}  ▶",
        app.selected_date.format("%A, %b %d, %Y"),
        today_tag,
        if !is_today { " [t=today]" } else { "" }
    );

    let total_dur_str = if app.data.is_all_away {
        format!("{} (All Away)", format_duration(app.data.total_usage_secs))
    } else {
        format_duration(app.data.total_usage_secs)
    };

    let mut wow_spans = Vec::new();
    if let Some(pct) = app.data.wow_delta_pct {
        if pct > 0.05 {
            wow_spans.push(Span::styled(
                format!(" ▲ +{:.1}%", pct),
                Style::default().fg(THEME_WHITE).add_modifier(Modifier::BOLD),
            ));
            wow_spans.push(Span::styled(" vs last week", Style::default().fg(THEME_MUTED)));
        } else if pct < -0.05 {
            wow_spans.push(Span::styled(
                format!(" ▼ {:.1}%", pct),
                Style::default().fg(THEME_WHITE).add_modifier(Modifier::BOLD),
            ));
            wow_spans.push(Span::styled(" vs last week", Style::default().fg(THEME_MUTED)));
        } else {
            wow_spans.push(Span::styled("  0.0% vs last week", Style::default().fg(THEME_MUTED)));
        }
    } else if app.data.total_usage_secs > 0 {
        wow_spans.push(Span::styled("  ▲ New activity", Style::default().fg(THEME_WHITE)));
    } else {
        wow_spans.push(Span::styled("  No activity recorded", Style::default().fg(THEME_MUTED)));
    }

    let left_lines = vec![
        Line::from(vec![
            Span::styled("Daily Usage  ", Style::default().fg(THEME_MUTED)),
            Span::styled(
                total_dur_str,
                Style::default().fg(THEME_TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
        ].into_iter().chain(wow_spans.into_iter()).collect::<Vec<Span>>()),
        Line::from(vec![
            Span::styled(date_str, Style::default().fg(THEME_TEXT).add_modifier(Modifier::BOLD)),
        ]),
    ];

    let left_para = Paragraph::new(left_lines).alignment(Alignment::Left);
    f.render_widget(left_para, header_layout[0]);

    // Right: Quick Status & Next auto-refresh
    let elapsed = app.last_refresh.elapsed().as_secs();
    let next_refresh = app.refresh_interval.as_secs().saturating_sub(elapsed);

    let mut right_lines = vec![
        Line::from(vec![
            Span::styled("actwatch ", Style::default().fg(THEME_WHITE).add_modifier(Modifier::BOLD)),
            Span::styled("TUI [B&W]", Style::default().fg(THEME_MUTED)),
            Span::styled(format!("  •  Auto-refresh in {}s", next_refresh), Style::default().fg(THEME_MUTED)),
        ]),
    ];

    if let Some((msg, _)) = &app.status_message {
        right_lines.push(Line::from(vec![
            Span::styled("ℹ ", Style::default().fg(THEME_WHITE)),
            Span::styled(msg, Style::default().fg(THEME_TEXT)),
        ]));
    } else if let Some(err) = &app.error_message {
        right_lines.push(Line::from(vec![
            Span::styled("⚠ ", Style::default().fg(THEME_WHITE)),
            Span::styled(err, Style::default().fg(THEME_TEXT)),
        ]));
    } else {
        right_lines.push(Line::from(vec![
            Span::styled("DB: ", Style::default().fg(THEME_MUTED)),
            Span::styled(
                app.db.path().file_name().unwrap_or_default().to_string_lossy(),
                Style::default().fg(THEME_MUTED),
            ),
        ]));
    }

    let right_para = Paragraph::new(right_lines).alignment(Alignment::Right);
    f.render_widget(right_para, header_layout[1]);
}

fn render_middle_section(f: &mut Frame, app: &App, area: Rect) {
    let middle_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(55), // 7-day Stacked Bar Chart
            Constraint::Percentage(45), // Category Breakdown + Away Card
        ])
        .split(area);

    render_stacked_bar_chart(f, app, middle_layout[0]);

    let right_middle_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10), // Category Breakdown (6 categories)
            Constraint::Length(4),  // Dedicated Away Card
        ])
        .split(middle_layout[1]);

    render_category_breakdown(f, app, right_middle_layout[0]);
    render_away_card(f, app, right_middle_layout[1]);
}

/// Custom 7-Day Stacked Bar Chart renderer using category gray shades and fill patterns.
fn render_stacked_bar_chart(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" 7-Day Activity (S - S) ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(THEME_HAIRLINE))
        .style(Style::default().bg(THEME_SURFACE));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width < 14 || inner.height < 4 {
        return;
    }

    let chart_widget = StackedBarWidget {
        chart: &app.data.chart,
    };
    f.render_widget(chart_widget, inner);
}

struct StackedBarWidget<'a> {
    chart: &'a crate::db::WeeklyChartData,
}

impl<'a> Widget for StackedBarWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 7 || area.height < 4 {
            return;
        }

        let days = &self.chart.days;
        if days.is_empty() {
            return;
        }

        let chart_top_y = area.y;
        let chart_bottom_y = area.y + area.height.saturating_sub(2); // 2 rows for Day label + Date number
        let available_height = (chart_bottom_y - chart_top_y) as usize;

        if available_height == 0 {
            return;
        }

        // Find max total secs for scale
        let max_secs = self
            .chart
            .max_daily_secs
            .max(self.chart.avg_daily_secs)
            .max(3600); // at least 1 hour scale

        // Draw average line
        if self.chart.avg_daily_secs > 0 {
            let avg_ratio = (self.chart.avg_daily_secs as f64 / max_secs as f64).clamp(0.0, 1.0);
            let avg_row = chart_bottom_y.saturating_sub((avg_ratio * available_height as f64).round() as u16);
            if avg_row >= chart_top_y && avg_row < chart_bottom_y {
                let avg_label = format!("┄ avg {} ┄", format_duration(self.chart.avg_daily_secs));
                let mut label_written = false;
                for x in area.x..area.x + area.width {
                    if !label_written && x + (avg_label.len() as u16) <= area.x + area.width - 1 {
                        buf.set_string(
                            x,
                            avg_row,
                            &avg_label,
                            Style::default().fg(THEME_MUTED),
                        );
                        label_written = true;
                    } else if !label_written {
                        buf[(x, avg_row)]
                            .set_char('┄')
                            .set_style(Style::default().fg(THEME_HAIRLINE));
                    }
                }
            }
        }

        // Each day column width
        let col_width = (area.width / 7).max(1);
        let bar_width = (col_width.saturating_sub(1)).max(1).min(6);

        for (i, day) in days.iter().enumerate() {
            let col_x = area.x + (i as u16 * col_width) + (col_width.saturating_sub(bar_width) / 2);
            let is_selected = day.is_selected;

            // Draw stacked segments
            if day.total_secs > 0 {
                let mut current_y = chart_bottom_y;

                for cat in Category::all() {
                    let cat_secs = *day.category_secs.get(&cat).unwrap_or(&0);
                    if cat_secs == 0 {
                        continue;
                    }

                    let cat_height = ((cat_secs as f64 / max_secs as f64) * available_height as f64).round() as u16;
                    let segment_height = cat_height.max(1);

                    for _ in 0..segment_height {
                        if current_y <= chart_top_y {
                            break;
                        }
                        current_y -= 1;

                        for bx in 0..bar_width {
                            let draw_x = col_x + bx;
                            if draw_x < area.x + area.width {
                                buf[(draw_x, current_y)]
                                    .set_char(theme::fill_char(cat))
                                    .set_style(Style::default().fg(theme::gray_shade(cat)));
                            }
                        }
                    }
                }
            } else if day.away_secs > 0 {
                // Day with away time only -> muted hatch dot at base
                for bx in 0..bar_width {
                    let draw_x = col_x + bx;
                    if draw_x < area.x + area.width {
                        buf[(draw_x, chart_bottom_y.saturating_sub(1))]
                            .set_char('▨')
                            .set_style(Style::default().fg(THEME_MUTED));
                    }
                }
            } else {
                // Completely empty day base dot
                for bx in 0..bar_width {
                    let draw_x = col_x + bx;
                    if draw_x < area.x + area.width {
                        buf[(draw_x, chart_bottom_y.saturating_sub(1))]
                            .set_char('·')
                            .set_style(Style::default().fg(THEME_HAIRLINE));
                    }
                }
            }

            // Day label (S, M, T, W, T, F, S)
            let day_name_style = if is_selected {
                Style::default().fg(THEME_WHITE).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default().fg(THEME_TEXT)
            };

            let label_x = col_x;
            if label_x < area.x + area.width {
                let name_short = &day.day_name[0..1.min(day.day_name.len())];
                buf.set_string(label_x, chart_bottom_y, name_short, day_name_style);
            }

            // Date number (e.g. "10")
            let date_num = format!("{}", day.date.day());
            let date_style = if is_selected {
                Style::default().fg(THEME_WHITE).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(THEME_MUTED)
            };

            if label_x < area.x + area.width {
                buf.set_string(label_x, chart_bottom_y + 1, &date_num, date_style);
            }
        }
    }
}

fn render_category_breakdown(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Category Breakdown ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(THEME_HAIRLINE))
        .style(Style::default().bg(THEME_SURFACE));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines = Vec::new();

    for cat_total in &app.data.category_breakdown {
        let cat = cat_total.category;
        let shade = theme::gray_shade(cat);
        let fill_ch = theme::fill_char(cat);
        let dur_str = format_duration(cat_total.total_secs);
        let pct_str = format!("{:.1}%", cat_total.percentage);

        // Patterned progress bar
        let bar_width: usize = 8;
        let filled = ((cat_total.percentage / 100.0) * bar_width as f64).round() as usize;
        let empty = bar_width.saturating_sub(filled);
        let bar_filled: String = std::iter::repeat(fill_ch).take(filled).collect();
        let bar_empty: String = " ".repeat(empty);

        lines.push(Line::from(vec![
            Span::styled(format!("{} ", fill_ch), Style::default().fg(shade)),
            Span::styled(
                format!("{:<23}", theme::name(cat)),
                Style::default().fg(THEME_TEXT),
            ),
            Span::styled(
                format!("{:>7} ", dur_str),
                Style::default().fg(THEME_TEXT),
            ),
            Span::styled(bar_filled, Style::default().fg(shade)),
            Span::styled(bar_empty, Style::default().fg(THEME_HAIRLINE)),
            Span::styled(format!(" {:>5}", pct_str), Style::default().fg(THEME_MUTED)),
        ]));
    }

    lines.push(Line::from(vec![
        Span::styled("Total Active Usage: ", Style::default().fg(THEME_MUTED)),
        Span::styled(
            format_duration(app.data.total_usage_secs),
            Style::default().fg(THEME_TEXT).add_modifier(Modifier::BOLD),
        ),
    ]));

    let para = Paragraph::new(lines).alignment(Alignment::Left);
    f.render_widget(para, inner);
}

/// Dedicated card for Away / Idle time, excluded from app totals.
fn render_away_card(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(THEME_HAIRLINE))
        .style(Style::default().bg(THEME_SURFACE));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let away_dur_str = format_duration(app.data.away_secs);

    let lines = vec![
        Line::from(vec![
            Span::styled("▨ ", Style::default().fg(THEME_MUTED)),
            Span::styled("Away / Idle: ", Style::default().fg(THEME_MUTED)),
            Span::styled(
                away_dur_str,
                Style::default().fg(THEME_TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  (Excluded from app totals)", Style::default().fg(THEME_MUTED)),
        ]),
        Line::from(vec![
            Span::styled(
                "  Hatched spans indicate screen lock / idle detection.",
                Style::default().fg(THEME_MUTED),
            ),
        ]),
    ];

    let para = Paragraph::new(lines).alignment(Alignment::Left);
    f.render_widget(para, inner);
}

fn render_app_table(f: &mut Frame, app: &mut App, area: Rect) {
    let title = format!(
        " Applications & Resources ({} apps) ",
        app.data.apps.len()
    );

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(THEME_HAIRLINE))
        .style(Style::default().bg(THEME_SURFACE));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Search / Filter & Sort Controls
            Constraint::Min(4),    // Table
        ])
        .split(inner);

    // Filter and Sort Control line
    let sort_dir_str = match app.sort_order {
        SortOrder::Descending => "▼",
        SortOrder::Ascending => "▲",
    };

    let sort_style = |mode: SortMode| -> Style {
        if app.sort_mode == mode {
            Style::default().fg(THEME_WHITE).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(THEME_MUTED)
        }
    };

    let search_style = if app.mode == AppMode::Search {
        Style::default().fg(THEME_WHITE).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(THEME_MUTED)
    };

    let search_display = if app.search_query.is_empty() {
        if app.mode == AppMode::Search {
            "🔍 [Type to search...]".to_string()
        } else {
            "🔍 [/] Search".to_string()
        }
    } else {
        format!("🔍 Filter: \"{}\" [Esc to clear]", app.search_query)
    };

    let sort_line = Line::from(vec![
        Span::styled(search_display, search_style),
        Span::raw("    Sort: "),
        Span::styled(
            format!("[1] Time {}", if app.sort_mode == SortMode::Time { sort_dir_str } else { "" }),
            sort_style(SortMode::Time),
        ),
        Span::raw("  "),
        Span::styled(
            format!("[2] Avg CPU {}", if app.sort_mode == SortMode::Cpu { sort_dir_str } else { "" }),
            sort_style(SortMode::Cpu),
        ),
        Span::raw("  "),
        Span::styled(
            format!("[3] Peak RSS {}", if app.sort_mode == SortMode::Memory { sort_dir_str } else { "" }),
            sort_style(SortMode::Memory),
        ),
        Span::raw("  "),
        Span::styled(
            format!("[4] Avg GPU {}", if app.sort_mode == SortMode::Gpu { sort_dir_str } else { "" }),
            sort_style(SortMode::Gpu),
        ),
    ]);

    let sort_para = Paragraph::new(sort_line);
    f.render_widget(sort_para, chunks[0]);

    // Table Content
    let filtered_apps = app.get_filtered_and_sorted_apps();

    if filtered_apps.is_empty() {
        let no_apps_msg = if app.data.is_all_away {
            "No active application usage recorded for this day (all away)."
        } else if app.search_query.is_empty() {
            "No application activity recorded for this day."
        } else {
            "No applications match the search filter."
        };

        let empty_p = Paragraph::new(no_apps_msg)
            .style(Style::default().fg(THEME_MUTED))
            .alignment(Alignment::Center);
        f.render_widget(empty_p, chunks[1]);
        return;
    }

    let max_app_time = filtered_apps.first().map(|a| a.duration_secs).unwrap_or(1).max(1);

    let header_row = Row::new(vec![
        "App / Process",
        "Category",
        "Time",
        "Activity Share",
        "Avg CPU %",
        "Peak RSS",
        "Avg GPU %",
    ])
    .style(
        Style::default()
            .fg(THEME_TEXT)
            .add_modifier(Modifier::BOLD)
            .bg(THEME_HAIRLINE),
    )
    .bottom_margin(1);

    let rows: Vec<Row> = filtered_apps
        .iter()
        .enumerate()
        .map(|(idx, app_row)| {
            let is_selected = idx == app.selected_app_index;
            let shade = theme::gray_shade(app_row.category);
            let fill_ch = theme::fill_char(app_row.category);

            // Share gauge
            let share_pct = (app_row.duration_secs as f64 / max_app_time as f64).clamp(0.0, 1.0);
            let bar_len: usize = 10;
            let filled = (share_pct * bar_len as f64).round() as usize;
            let empty = bar_len.saturating_sub(filled);
            let share_filled: String = std::iter::repeat(fill_ch).take(filled).collect();
            let share_empty: String = "░".repeat(empty);

            let app_name_display = if app_row.display_name.len() > 24 {
                format!("{}…", &app_row.display_name[0..23])
            } else {
                app_row.display_name.clone()
            };

            let row_style = if is_selected {
                Style::default()
                    .bg(THEME_SELECTED)
                    .fg(THEME_WHITE)
                    .add_modifier(Modifier::BOLD)
            } else if idx % 2 == 0 {
                Style::default().bg(THEME_SURFACE)
            } else {
                Style::default().bg(THEME_ROW_ALT)
            };

            Row::new(vec![
                Line::from(vec![
                    Span::styled(format!("{} ", fill_ch), Style::default().fg(shade)),
                    Span::styled(app_name_display, Style::default().fg(THEME_TEXT)),
                ]),
                Line::from(Span::styled(
                    theme::short_name(app_row.category),
                    Style::default().fg(shade),
                )),
                Line::from(Span::styled(
                    format_duration(app_row.duration_secs),
                    Style::default().fg(THEME_TEXT),
                )),
                Line::from(vec![
                    Span::styled(share_filled, Style::default().fg(shade)),
                    Span::styled(share_empty, Style::default().fg(THEME_HAIRLINE)),
                ]),
                Line::from(Span::styled(
                    format_pct(app_row.avg_cpu),
                    Style::default().fg(THEME_MUTED),
                )),
                Line::from(Span::styled(
                    format_rss(app_row.peak_rss_kb),
                    Style::default().fg(THEME_MUTED),
                )),
                Line::from(Span::styled(
                    format_pct(app_row.avg_gpu),
                    Style::default().fg(THEME_MUTED),
                )),
            ])
            .style(row_style)
        })
        .collect();

    let widths = [
        Constraint::Percentage(26), // App
        Constraint::Percentage(16), // Category
        Constraint::Percentage(14), // Time
        Constraint::Percentage(16), // Activity Share
        Constraint::Percentage(10), // CPU
        Constraint::Percentage(10), // RSS
        Constraint::Percentage(8),  // GPU
    ];

    let table = Table::new(rows, widths)
        .header(header_row)
        .row_highlight_style(
            Style::default()
                .bg(THEME_SELECTED)
                .fg(THEME_WHITE)
                .add_modifier(Modifier::BOLD),
        );

    let mut state = TableState::default();
    state.select(Some(app.selected_app_index));
    f.render_stateful_widget(table, chunks[1], &mut state);
}

fn render_footer(f: &mut Frame, app: &App, area: Rect) {
    let mode_indicator = if app.mode == AppMode::Search {
        Span::styled(" [SEARCH MODE] ", Style::default().bg(THEME_WHITE).fg(THEME_BG).add_modifier(Modifier::BOLD))
    } else {
        Span::styled(" [NORMAL] ", Style::default().bg(THEME_SELECTED).fg(THEME_WHITE).add_modifier(Modifier::BOLD))
    };

    let help_line = Line::from(vec![
        mode_indicator,
        Span::raw(" "),
        Span::styled("q", Style::default().fg(THEME_WHITE).add_modifier(Modifier::BOLD)),
        Span::styled(" quit  ", Style::default().fg(THEME_MUTED)),
        Span::styled("←/→/h/l", Style::default().fg(THEME_WHITE).add_modifier(Modifier::BOLD)),
        Span::styled(" day  ", Style::default().fg(THEME_MUTED)),
        Span::styled("t", Style::default().fg(THEME_WHITE).add_modifier(Modifier::BOLD)),
        Span::styled(" today  ", Style::default().fg(THEME_MUTED)),
        Span::styled("[/]", Style::default().fg(THEME_WHITE).add_modifier(Modifier::BOLD)),
        Span::styled(" week  ", Style::default().fg(THEME_MUTED)),
        Span::styled("/", Style::default().fg(THEME_WHITE).add_modifier(Modifier::BOLD)),
        Span::styled(" search  ", Style::default().fg(THEME_MUTED)),
        Span::styled("1-4", Style::default().fg(THEME_WHITE).add_modifier(Modifier::BOLD)),
        Span::styled(" sort  ", Style::default().fg(THEME_MUTED)),
        Span::styled("s", Style::default().fg(THEME_WHITE).add_modifier(Modifier::BOLD)),
        Span::styled(" cycle  ", Style::default().fg(THEME_MUTED)),
        Span::styled("r", Style::default().fg(THEME_WHITE).add_modifier(Modifier::BOLD)),
        Span::styled(" refresh  ", Style::default().fg(THEME_MUTED)),
        Span::styled("↑/↓/jk", Style::default().fg(THEME_WHITE).add_modifier(Modifier::BOLD)),
        Span::styled(" scroll", Style::default().fg(THEME_MUTED)),
    ]);

    let footer_para = Paragraph::new(help_line).style(Style::default().bg(THEME_BG));
    f.render_widget(footer_para, area);
}
