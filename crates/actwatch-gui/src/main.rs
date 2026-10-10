use actwatch_core::Store;
use chrono::{Datelike, Duration, Local, NaiveDate};
use iced::widget::canvas::{self, Canvas, Geometry, Stroke};
use iced::widget::{
    button, column, container, pick_list, row, scrollable, text, text_input, Rule, Space,
};
use iced::{
    Alignment, Color, Element, Length, Point, Rectangle, Size, Subscription, Task, Theme
};
use std::collections::HashMap;

mod shim;
use shim::{Categories, Category, DisplayNames};

pub fn main() -> iced::Result {
    iced::application("ActWatch", ActWatch::update, ActWatch::view)
        .theme(|_| Theme::Dark)
        .subscription(ActWatch::subscription)
        .run()
}

#[derive(Debug, Clone)]
pub enum Message {
    Refresh,
    DeviceSelected(String),
    PeriodSelected(String),
    DatePrev,
    DateNext,
    AppCategorySelected(String),
    SearchChanged(String),
}

#[derive(Debug, Clone)]
pub struct AppUsage {
    pub app_id: String,
    pub time_secs: i64,
    pub avg_cpu: f64,
    pub peak_rss_kb: i64,
    pub avg_gpu: f64,
}

struct ActWatch {
    store: Store,
    cats: Categories,
    names: DisplayNames,

    current_date: NaiveDate,
    day_usage: Vec<AppUsage>,
    away_secs: i64,
    weekly_summary: Vec<(NaiveDate, HashMap<Category, i64>, i64)>,

    device: String,
    period: String,
    app_category: String,
    search_query: String,
}

impl Default for ActWatch {
    fn default() -> Self {
        let db_path = dirs::data_local_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("~/.local/share"))
            .join("actwatch")
            .join("activity.db");

        let store = Store::open_or_create(&db_path).unwrap_or_else(|_| Store::open_in_memory().unwrap());

        let mut app = Self {
            store,
            cats: Categories::new(),
            names: DisplayNames::load(),
            current_date: Local::now().naive_local().date(),
            day_usage: Vec::new(),
            away_secs: 0,
            weekly_summary: Vec::new(),
            device: "All Devices".to_string(),
            period: "This Week".to_string(),
            app_category: "All Usage".to_string(),
            search_query: String::new(),
        };
        app.load_data();
        app
    }
}

// B&W Theme Colors
const COLOR_BG: Color = Color::from_rgb(0.043, 0.043, 0.047); // #0B0B0C
const COLOR_SURFACE: Color = Color::from_rgb(0.078, 0.078, 0.086); // #141416
const COLOR_TEXT: Color = Color::from_rgb(0.957, 0.957, 0.961); // #F4F4F5
const COLOR_MUTED: Color = Color::from_rgb(0.557, 0.557, 0.576); // #8E8E93

impl ActWatch {
    fn load_data(&mut self) {
        let day_str = self.current_date.format("%Y-%m-%d").to_string();
        self.day_usage.clear();
        self.away_secs = 0;

        if let Ok(rows) = self.store.read_rollup(&day_str) {
            for r in rows {
                if r.app_id == "away" {
                    self.away_secs += r.secs;
                } else {
                    self.day_usage.push(AppUsage {
                        app_id: r.app_id,
                        time_secs: r.secs,
                        avg_cpu: r.avg_cpu,
                        peak_rss_kb: r.peak_rss_kb,
                        avg_gpu: r.avg_gpu,
                    });
                }
            }
        }
        self.day_usage.sort_by(|a, b| b.time_secs.cmp(&a.time_secs));

        self.weekly_summary.clear();
        for i in (0..7).rev() {
            let d = self.current_date - Duration::days(i);
            let d_str = d.format("%Y-%m-%d").to_string();
            let mut summary = HashMap::new();
            for cat in Category::all() {
                summary.insert(cat, 0);
            }
            let mut away_secs = 0;

            if let Ok(rows) = self.store.read_rollup(&d_str) {
                for r in rows {
                    if r.app_id == "away" {
                        away_secs += r.secs;
                    } else {
                        let cat = self.cats.categorize(&r.app_id);
                        *summary.entry(cat).or_insert(0) += r.secs;
                    }
                }
            }
            self.weekly_summary.push((d, summary, away_secs));
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Refresh => self.load_data(),
            Message::DeviceSelected(d) => self.device = d,
            Message::PeriodSelected(p) => self.period = p,
            Message::DatePrev => {
                self.current_date -= Duration::days(1);
                self.load_data();
            }
            Message::DateNext => {
                self.current_date += Duration::days(1);
                self.load_data();
            }
            Message::AppCategorySelected(c) => self.app_category = c,
            Message::SearchChanged(q) => self.search_query = q,
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        iced::time::every(std::time::Duration::from_secs(30)).map(|_| Message::Refresh)
    }

    fn view(&self) -> Element<'_, Message> {
        let title_row = row![
            text("Device").size(16).color(COLOR_MUTED),
            pick_list(
                vec!["All Devices".to_string(), "This PC".to_string()],
                Some(self.device.clone()),
                Message::DeviceSelected
            )
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let total_secs: i64 = self.day_usage.iter().map(|u| u.time_secs).sum();
        let hrs = total_secs / 3600;
        let mins = (total_secs % 3600) / 60;

        let prev_week_date = self.current_date - Duration::days(7);
        let prev_week_str = prev_week_date.format("%Y-%m-%d").to_string();
        let prev_week_secs: i64 = self.store.read_rollup(&prev_week_str).unwrap_or_default().iter().filter(|r| r.app_id != "away").map(|r| r.secs).sum();
        
        let delta_pct = if prev_week_secs == 0 {
            if total_secs > 0 { "∞% from last week".to_string() } else { "0% from last week".to_string() }
        } else {
            let diff = total_secs - prev_week_secs;
            let pct = (diff as f64 / prev_week_secs as f64) * 100.0;
            format!("{pct:.0}% from last week")
        };

        let daily_usage_row = row![
            column![
                text("Daily Usage").size(16).color(COLOR_MUTED),
                text(format!("{}h {}m", hrs, mins)).size(40).color(COLOR_TEXT),
            ]
            .spacing(5),
            Space::with_width(Length::Fill),
            pick_list(
                vec!["This Week".to_string(), "Last Week".to_string()],
                Some(self.period.clone()),
                Message::PeriodSelected
            ),
            button("<").on_press(Message::DatePrev),
            text(if self.current_date == Local::now().naive_local().date() {
                "Today".to_string()
            } else {
                self.current_date.format("%a, %b %d").to_string()
            })
            .size(16).color(COLOR_TEXT),
            button(">").on_press(Message::DateNext),
            text(delta_pct).size(14).color(COLOR_MUTED)
        ]
        .spacing(15)
        .align_y(Alignment::Center);

        let mut legend_items: Vec<Element<'_, Message>> = Vec::new();
        let mut add_legend = |label: &str, shade: f32, is_away: bool| {
            let square = Canvas::new(LegendSwatch { shade, is_away }).width(12.0).height(12.0);
            legend_items.push(row![square, text(label.to_string()).size(14).color(COLOR_TEXT)].spacing(5).align_y(Alignment::Center).into());
        };

        add_legend("Social", 1.0, false);
        add_legend("Productivity & Finance", 0.8, false);
        add_legend("Utilities", 0.6, false);
        add_legend("Gaming", 0.4, false);
        add_legend("Media", 0.3, false);
        add_legend("Other", 0.15, false);
        add_legend("Away", 0.4, true);

        let mut legend = iced::widget::Column::new().spacing(10);
        let mut current_row = Vec::new();
        for item in legend_items {
            current_row.push(item);
            if current_row.len() == 4 {
                legend = legend.push(iced::widget::Row::with_children(current_row).spacing(20));
                current_row = Vec::new();
            }
        }
        if !current_row.is_empty() {
            legend = legend.push(iced::widget::Row::with_children(current_row).spacing(20));
        }

        let chart_content = ContainerChartView {
            summary: self.weekly_summary.clone(),
        };

        let mut days_row = iced::widget::Row::new().width(Length::Fill);
        for (d, _, _) in &self.weekly_summary {
            let s = match d.weekday() {
                chrono::Weekday::Mon => "M",
                chrono::Weekday::Tue => "T",
                chrono::Weekday::Wed => "W",
                chrono::Weekday::Thu => "T",
                chrono::Weekday::Fri => "F",
                chrono::Weekday::Sat => "S",
                chrono::Weekday::Sun => "S",
            };
            days_row = days_row.push(
                container(text(s).size(14).color(COLOR_MUTED))
                    .width(Length::FillPortion(1))
                    .center_x(Length::Fill),
            );
        }

        let top_card = container(
            column![
                title_row,
                daily_usage_row,
                Space::with_height(10),
                container(Canvas::new(chart_content).width(Length::Fill).height(Length::Fixed(150.0)))
                    .padding(10),
                days_row,
                Space::with_height(10),
                legend
            ]
            .spacing(10),
        )
        .padding(20)
        .style(|_theme| container::background(COLOR_SURFACE).into());

        let away_hrs = self.away_secs / 3600;
        let away_mins = (self.away_secs % 3600) / 60;
        let away_card = container(
            row![
                text("Away").size(16).color(COLOR_MUTED),
                Space::with_width(Length::Fill),
                text(format!("{}h {}m", away_hrs, away_mins)).size(16).color(COLOR_MUTED)
            ].align_y(Alignment::Center)
        ).padding(20).style(|_theme| container::background(COLOR_SURFACE).into());

        let app_controls = row![
            text("Show Apps").size(16).color(COLOR_TEXT),
            pick_list(
                vec!["All Usage".to_string(), "Limits".to_string()],
                Some(self.app_category.clone()),
                Message::AppCategorySelected
            ),
            Space::with_width(Length::Fill),
            text_input("Search apps...", &self.search_query)
                .on_input(Message::SearchChanged)
                .width(Length::Fixed(200.0))
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let table_header = row![
            text("Apps").width(Length::FillPortion(3)).color(COLOR_MUTED),
            text("Time").width(Length::FillPortion(1)).color(COLOR_MUTED),
            text("CPU %").width(Length::FillPortion(1)).color(COLOR_MUTED),
            text("RAM").width(Length::FillPortion(1)).color(COLOR_MUTED),
            text("GPU %").width(Length::FillPortion(1)).color(COLOR_MUTED),
        ]
        .spacing(10);

        let mut filtered_apps = self.day_usage.clone();
        if !self.search_query.is_empty() {
            let q = self.search_query.to_lowercase();
            filtered_apps.retain(|u| self.names.resolve(&u.app_id).to_lowercase().contains(&q));
        }

        let mut app_rows = column![table_header, Rule::horizontal(1)].spacing(10);

        if filtered_apps.is_empty() && self.away_secs == 0 {
             app_rows = app_rows.push(text("No activity recorded for this period.").color(COLOR_MUTED));
        } else {
            for app in filtered_apps {
                let h = app.time_secs / 3600;
                let m = (app.time_secs % 3600) / 60;
                app_rows = app_rows.push(
                    row![
                        text(self.names.resolve(&app.app_id)).width(Length::FillPortion(3)).color(COLOR_TEXT),
                        text(format!("{}h {}m", h, m)).width(Length::FillPortion(1)).color(COLOR_TEXT),
                        text(format!("{:.1}%", app.avg_cpu)).width(Length::FillPortion(1)).color(COLOR_MUTED),
                        text(format!("{} MB", app.peak_rss_kb / 1024)).width(Length::FillPortion(1)).color(COLOR_MUTED),
                        text(format!("{:.1}%", app.avg_gpu)).width(Length::FillPortion(1)).color(COLOR_MUTED),
                    ]
                    .spacing(10),
                );
            }
        }

        let bottom_card = container(
            column![app_controls, Space::with_height(10), scrollable(app_rows)].spacing(10),
        )
        .padding(20)
        .style(|_theme| container::background(COLOR_SURFACE).into());

        container(
            column![top_card, away_card, bottom_card].spacing(20).padding(20),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_theme| container::background(COLOR_BG).into())
        .into()
    }
}

struct LegendSwatch {
    shade: f32,
    is_away: bool,
}

impl canvas::Program<Message> for LegendSwatch {
    type State = ();
    fn draw(&self, _state: &Self::State, renderer: &iced::Renderer, _theme: &Theme, bounds: Rectangle, _cursor: iced::mouse::Cursor) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        
        if self.is_away {
            frame.fill_rectangle(Point::ORIGIN, bounds.size(), Color::from_rgb(0.2, 0.2, 0.2));
            let mut p = canvas::path::Builder::new();
            for i in 0..=(bounds.width as i32 + bounds.height as i32) {
                if i % 3 == 0 {
                    p.move_to(Point::new(i as f32, 0.0));
                    p.line_to(Point::new(0.0, i as f32));
                }
            }
            frame.stroke(&p.build(), Stroke::default().with_color(COLOR_MUTED).with_width(1.0));
        } else {
            frame.fill_rectangle(Point::ORIGIN, bounds.size(), Color::from_rgb(self.shade, self.shade, self.shade));
        }
        
        vec![frame.into_geometry()]
    }
}

struct ContainerChartView {
    summary: Vec<(NaiveDate, HashMap<Category, i64>, i64)>,
}

impl canvas::Program<Message> for ContainerChartView {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        let days_count = 7;
        let bar_width = 30.0;
        let spacing = (bounds.width - (days_count as f32 * bar_width)) / (days_count as f32 + 1.0);

        let max_secs = self
            .summary
            .iter()
            .map(|(_, s, away)| s.values().sum::<i64>() + away)
            .max()
            .unwrap_or(3600 * 8);

        let max_secs = (max_secs as f32).max(3600.0);
        let height_scale = bounds.height / max_secs;

        for (i, (_date, summary, away_secs)) in self.summary.iter().enumerate() {
            let x = spacing + (i as f32) * (bar_width + spacing);
            let mut current_y = bounds.height;

            let categories = [
                (Category::Social, 1.0, false),               
                (Category::ProductivityFinance, 0.8, false),  
                (Category::Utilities, 0.6, true),             
                (Category::Gaming, 0.4, false),               
                (Category::Media, 0.3, true),                 
                (Category::Other, 0.15, false),               
            ];

            let draw_bar = |frame: &mut canvas::Frame, x, y, w, h, shade: f32, texture: bool| {
                frame.fill_rectangle(Point::new(x, y), Size::new(w, h), Color::from_rgb(shade, shade, shade));
                if texture {
                    let mut p = canvas::path::Builder::new();
                    let lines = (h / 4.0) as i32;
                    for l in 0..lines {
                        let ly = y + (l as f32 * 4.0);
                        p.move_to(Point::new(x, ly));
                        p.line_to(Point::new(x + w, ly));
                    }
                    frame.stroke(&p.build(), Stroke::default().with_color(Color::from_rgb(shade * 0.7, shade * 0.7, shade * 0.7)).with_width(1.0));
                }
            };

            for (cat, shade, texture) in categories.iter() {
                if let Some(&secs) = summary.get(cat) {
                    if secs > 0 {
                        let h = secs as f32 * height_scale;
                        current_y -= h;
                        draw_bar(&mut frame, x, current_y, bar_width, h, *shade, *texture);
                    }
                }
            }

            if *away_secs > 0 {
                let h = *away_secs as f32 * height_scale;
                current_y -= h;
                frame.fill_rectangle(Point::new(x, current_y), Size::new(bar_width, h), Color::from_rgb(0.2, 0.2, 0.2));
                let mut p = canvas::path::Builder::new();
                for l in 0..=(bar_width as i32 + h as i32) {
                    if l % 3 == 0 {
                        let start_x = x + (l as f32).min(bar_width);
                        let start_y = current_y + (l as f32 - bar_width).max(0.0);
                        let end_x = x + (l as f32 - h).max(0.0);
                        let end_y = current_y + (l as f32).min(h);
                        p.move_to(Point::new(start_x, start_y));
                        p.line_to(Point::new(end_x, end_y));
                    }
                }
                frame.stroke(&p.build(), Stroke::default().with_color(COLOR_MUTED).with_width(1.0));
            }
        }

        vec![frame.into_geometry()]
    }
}
