use anyhow::Result;
use chrono::NaiveDate;
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, stdout};
use std::path::PathBuf;
use std::time::Duration;

mod app;
pub mod core_shim;
mod db;
mod ui;

use app::App;
use ui::render_ui;

struct CliArgs {
    db_path: Option<PathBuf>,
    date: Option<NaiveDate>,
}

fn parse_args() -> Result<Option<CliArgs>> {
    let mut args = std::env::args().skip(1);
    let mut db_path = None;
    let mut date = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" | "-d" => {
                if let Some(val) = args.next() {
                    db_path = Some(PathBuf::from(val));
                } else {
                    eprintln!("Error: --db requires a file path");
                    std::process::exit(1);
                }
            }
            "--date" => {
                if let Some(val) = args.next() {
                    match NaiveDate::parse_from_str(&val, "%Y-%m-%d") {
                        Ok(d) => date = Some(d),
                        Err(e) => {
                            eprintln!("Error parsing date '{}' (expected YYYY-MM-DD): {}", val, e);
                            std::process::exit(1);
                        }
                    }
                } else {
                    eprintln!("Error: --date requires YYYY-MM-DD");
                    std::process::exit(1);
                }
            }
            "--help" | "-h" => {
                println!("actwatch-tui — Terminal Activity Monitor (Monochrome B&W)");
                println!();
                println!("USAGE:");
                println!("    actwatch-tui [OPTIONS]");
                println!();
                println!("OPTIONS:");
                println!("    -d, --db <PATH>      Path to SQLite activity.db");
                println!("        --date <DATE>    Initial date to inspect (YYYY-MM-DD)");
                println!("    -h, --help           Show this help message");
                println!();
                println!("KEYBINDINGS:");
                println!("    q, Ctrl+C            Quit application");
                println!("    ← / →, h / l         Navigate previous / next day");
                println!("    t                    Jump to Today");
                println!("    [ / ]                Navigate previous / next week (7 days)");
                println!("    /                    Search / filter applications");
                println!("    Esc                  Clear filter or exit search");
                println!("    1, 2, 3, 4           Sort by Time / Avg CPU / Peak RSS / Avg GPU");
                println!("    s                    Cycle sort column");
                println!("    r                    Force refresh from database");
                println!("    ↑ / ↓, k / j         Scroll application table (1 item)");
                println!("    PageUp / PageDown    Scroll application table (10 items)");
                println!("    Home / End, g / G    Scroll to first / last item");
                return Ok(None);
            }
            _ => {
                eprintln!("Unknown argument: {}", arg);
                eprintln!("Run with --help for usage.");
                std::process::exit(1);
            }
        }
    }

    Ok(Some(CliArgs { db_path, date }))
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn restore_terminal(mut terminal: Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn main() -> Result<()> {
    let args = match parse_args()? {
        Some(a) => a,
        None => return Ok(()),
    };

    // Install panic hook to restore terminal cleanly on crash
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen);
        original_hook(panic_info);
    }));

    let mut app = App::new(args.db_path, args.date)?;
    let mut terminal = setup_terminal()?;

    let res = run_app(&mut terminal, &mut app);

    restore_terminal(terminal)?;

    if let Err(err) = res {
        eprintln!("Application error: {:?}", err);
    }

    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| render_ui(f, app))?;

        // Poll for events up to 250ms
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                // Only process press events (ignore release/repeat on Windows/crossterm)
                if key.kind == KeyEventKind::Press {
                    app.on_key(key);
                }
            }
        }

        app.tick();

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
