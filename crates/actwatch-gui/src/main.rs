use iced::widget::{button, column, row, text, container, pick_list, text_input, scrollable, Space, Rule, canvas, Component};
use iced::widget::canvas::{Canvas, Cursor, Path, Stroke, Geometry, LineDash};
use iced::{Alignment, Element, Length, Theme, Color, Font, Task, application, Subscription, Rectangle, Point, Size};
use chrono::{Local, NaiveDate, Duration, Datelike};
use std::collections::HashMap;

mod db;
use db::{Db, Category, AppUsage};

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

struct ActWatch {
    db: Db,
    current_date: NaiveDate,
    day_usage: Vec<AppUsage>,
    weekly_summary: Vec<(NaiveDate, HashMap<Category, u64>)>,
    
    device: String,
    period: String,
    app_category: String,
    search_query: String,
}

impl Default for ActWatch {
    fn default() -> Self {
        let db = Db::open().unwrap_or_else(|_| Db::open().unwrap());
        let mut app = Self {
            db,
            current_date: Local::now().naive_local().date(),
            day_usage: Vec::new(),
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

impl ActWatch {
    fn load_data(&mut self) {
        self.day_usage = self.db.get_day_usage(self.current_date).unwrap_or_default();
        self.weekly_summary = self.db.get_weekly_summary(self.current_date).unwrap_or_default();
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Refresh => {
                self.load_data();
            }
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

    fn view(&self) -> Element<Message> {
        let title_row = row![
            text("Device").size(16).color(Color::from_rgb(0.6, 0.6, 0.6)),
            pick_list(
                vec!["All Devices".to_string(), "This PC".to_string()],
                Some(self.device.clone()),
                Message::DeviceSelected
            )
        ].spacing(10).align_y(Alignment::Center);

        let total_secs: u64 = self.day_usage.iter().map(|u| u.time_secs).sum();
        let hrs = total_secs / 3600;
        let mins = (total_secs % 3600) / 60;
        
        let daily_usage_row = row![
            column![
                text("Daily Usage").size(16).color(Color::from_rgb(0.6, 0.6, 0.6)),
                text(format!("{}h {}m", hrs, mins)).size(40),
            ].spacing(5),
            Space::with_width(Length::Fill),
            pick_list(
                vec!["This Week".to_string(), "Last Week".to_string()],
                Some(self.period.clone()),
                Message::PeriodSelected
            ),
            button("<").on_press(Message::DatePrev),
            text(if self.current_date == Local::now().naive_local().date() { "Today".to_string() } else { self.current_date.format("%a, %b %d").to_string() }).size(16),
            button(">").on_press(Message::DateNext),
            text("0% from last week").size(14).color(Color::from_rgb(0.5, 0.5, 0.5))
        ].spacing(15).align_y(Alignment::Center);

        let legend = row![
            row![
                text("●").color(Color::from_rgb8(0, 150, 255)),
                text("Social").size(14)
            ].spacing(5),
            row![
                text("●").color(Color::from_rgb8(0, 255, 255)),
                text("Productivity & Finance").size(14)
            ].spacing(5),
            row![
                text("●").color(Color::from_rgb8(255, 165, 0)),
                text("Utilities").size(14)
            ].spacing(5)
        ].spacing(20);

        let chart_content = ContainerChartView {
            summary: self.weekly_summary.clone(),
        };

        // Create labels for x-axis
        let mut labels_row = row![].spacing(10);
        // We will just put an empty row and let canvas handle it, or align them manually.
        // Actually it's easier to let canvas draw if needed, but text in canvas is tedious.
        // Let's just do a row with fill spacing.
        let mut days_row = row![].width(Length::Fill);
        for (d, _) in &self.weekly_summary {
            let s = match d.weekday() {
                chrono::Weekday::Mon => "M", chrono::Weekday::Tue => "T",
                chrono::Weekday::Wed => "W", chrono::Weekday::Thu => "T",
                chrono::Weekday::Fri => "F", chrono::Weekday::Sat => "S",
                chrono::Weekday::Sun => "S",
            };
            days_row = days_row.push(
                container(text(s).size(14).color(Color::from_rgb(0.5, 0.5, 0.5)))
                    .width(Length::FillPortion(1))
                    .center_x(Length::Fill)
            );
        }

        let top_card = container(
            column![
                title_row,
                daily_usage_row,
                Space::with_height(10),
                container(Canvas::new(chart_content).width(Length::Fill).height(Length::Fixed(150.0))).padding(10),
                days_row,
                Space::with_height(10),
                legend
            ].spacing(10)
        )
        .padding(20)
        .style(|_theme| container::background(Color::from_rgb(0.1, 0.1, 0.12)).into());

        let app_controls = row![
            text("Show Apps").size(16),
            pick_list(
                vec!["All Usage".to_string(), "Limits".to_string()],
                Some(self.app_category.clone()),
                Message::AppCategorySelected
            ),
            Space::with_width(Length::Fill),
            text_input("Search apps...", &self.search_query).on_input(Message::SearchChanged).width(Length::Fixed(200.0))
        ].spacing(10).align_y(Alignment::Center);

        let table_header = row![
            text("Apps").width(Length::FillPortion(3)),
            text("Time").width(Length::FillPortion(1)),
            text("Limits").width(Length::FillPortion(1)),
            text("CPU %").width(Length::FillPortion(1)),
            text("RAM mb").width(Length::FillPortion(1)),
            text("GPU %").width(Length::FillPortion(1)),
        ].spacing(10);

        let mut filtered_apps = self.day_usage.clone();
        if !self.search_query.is_empty() {
            let q = self.search_query.to_lowercase();
            filtered_apps.retain(|u| u.app_id.to_lowercase().contains(&q));
        }

        let mut app_rows = column![table_header, Rule::horizontal(1)].spacing(10);
        
        for app in filtered_apps {
            let h = app.time_secs / 3600;
            let m = (app.time_secs % 3600) / 60;
            app_rows = app_rows.push(
                row![
                    text(&app.app_id).width(Length::FillPortion(3)),
                    text(format!("{}h {}m", h, m)).width(Length::FillPortion(1)),
                    text("-").width(Length::FillPortion(1)),
                    text(format!("{:.1}%", app.avg_cpu)).width(Length::FillPortion(1)),
                    text(format!("{}MB", app.peak_rss_kb / 1024)).width(Length::FillPortion(1)),
                    text(format!("{:.1}%", app.avg_gpu)).width(Length::FillPortion(1)),
                ].spacing(10)
            );
        }

        let bottom_card = container(
            column![
                app_controls,
                Space::with_height(10),
                scrollable(app_rows)
            ].spacing(10)
        )
        .padding(20)
        .style(|_theme| container::background(Color::from_rgb(0.1, 0.1, 0.12)).into());

        container(
            column![top_card, bottom_card].spacing(20).padding(20)
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}

struct ContainerChartView {
    summary: Vec<(NaiveDate, HashMap<Category, u64>)>,
}

impl canvas::Program<Message> for ContainerChartView {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        
        let days_count = 7;
        // Make spacing flexible
        let bar_width = 30.0;
        let spacing = (bounds.width - (days_count as f32 * bar_width)) / (days_count as f32 + 1.0);
        
        let max_secs = self.summary.iter().map(|(_, s)| {
            s.values().sum::<u64>()
        }).max().unwrap_or(3600 * 8);
        
        let max_secs = (max_secs as f32).max(3600.0);
        let height_scale = bounds.height / max_secs;

        let avg_secs = if self.summary.is_empty() { 0.0 } else { 
            let sum: u64 = self.summary.iter().map(|(_, s)| s.values().sum::<u64>()).sum();
            sum as f32 / self.summary.len() as f32 
        };
        
        let avg_y = bounds.height - (avg_secs * height_scale);
        let mut avg_path = canvas::path::Builder::new();
        avg_path.move_to(Point::new(0.0, avg_y));
        avg_path.line_to(Point::new(bounds.width, avg_y));
        
        // Removed LineDash since styling varies across iced versions, just drawing a solid translucent line
        frame.stroke(
            &avg_path.build(),
            Stroke::default().with_color(Color::from_rgba(0.8, 0.8, 0.8, 0.5)).with_width(2.0)
        );

        for (i, (_date, summary)) in self.summary.iter().enumerate() {
            let x = spacing + (i as f32) * (bar_width + spacing);
            let mut current_y = bounds.height;

            let categories = [
                (Category::Social, Color::from_rgb8(0, 150, 255)),
                (Category::Productivity, Color::from_rgb8(0, 255, 255)),
                (Category::Utilities, Color::from_rgb8(255, 165, 0)),
                (Category::Uncategorized, Color::from_rgb8(150, 150, 150)),
            ];

            for (cat, color) in categories.iter() {
                if let Some(&secs) = summary.get(cat) {
                    if secs > 0 {
                        let h = secs as f32 * height_scale;
                        current_y -= h;
                        // Draw stacked bar parts
                        frame.fill_rectangle(
                            Point::new(x, current_y),
                            Size::new(bar_width, h),
                            *color
                        );
                    }
                }
            }
        }

        vec![frame.into_geometry()]
    }
}
