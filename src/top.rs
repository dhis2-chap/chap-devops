//! `chap-devops top`: every chap-devops deployment on this machine as a tree that keeps
//! itself up to date - `chap-devops init` deployments and `chap-devops run` groups alike,
//! each with chap-core, its components and its models under it, and what each
//! container uses.
//!
//! Docker is asked from a thread of its own, so a slow `docker stats` never
//! holds up a key press; the screen draws whatever snapshot arrived last.

pub mod data;
pub mod record;
pub mod view;

use crate::commands::Ctx;
use crate::error::Result;
use crate::tui::theme::Theme;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;
use view::{App, Confirm, Entry, Logs};

/// How many log lines `l` shows.
const LOG_LINES: usize = 200;

/// Run the screen until `q`.
pub fn run(ctx: &Ctx, known: Vec<PathBuf>, interval: u64) -> Result<()> {
    let theme = Theme::detect();
    let mut terminal = ratatui::try_init().map_err(|e| {
        anyhow::anyhow!(
            "could not take over the terminal: {e}; `chap-devops top` needs an interactive \
             terminal, and `chap-devops ps` lists the same models in a script"
        )
    })?;
    let result = event_loop(ctx, &mut terminal, &theme, known, interval);
    ratatui::restore();
    result
}

fn event_loop(
    ctx: &Ctx,
    terminal: &mut ratatui::DefaultTerminal,
    theme: &Theme,
    known: Vec<PathBuf>,
    interval: u64,
) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    let now = Arc::new(AtomicBool::new(true));
    let quit = Arc::new(AtomicBool::new(false));
    {
        let (now, quit) = (now.clone(), quit.clone());
        std::thread::spawn(move || {
            while !quit.load(Ordering::Relaxed) {
                // Cleared before the look, not after: an `r` pressed while a
                // slow `docker stats` is in flight asks for one more.
                now.store(false, Ordering::Relaxed);
                let nodes = data::collect(&known);
                if tx.send((nodes, clock())).is_err() {
                    return;
                }
                for _ in 0..interval * 10 {
                    if quit.load(Ordering::Relaxed) || now.load(Ordering::Relaxed) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        });
    }

    let mut app = App {
        interval,
        loading: true,
        ..App::default()
    };
    // Logs are asked for on a thread of their own: `docker compose logs` has
    // no deadline, and a daemon that hangs must not take the keys with it.
    let (logs_tx, logs_rx) = mpsc::channel::<Logs>();
    let result = loop {
        while let Ok((nodes, at)) = rx.try_recv() {
            app.replace(nodes, at);
        }
        while let Ok(logs) = logs_rx.try_recv() {
            // Only into a pane still waiting for these logs.
            if app
                .logs
                .as_ref()
                .is_some_and(|open| open.title == logs.title)
            {
                app.logs = Some(logs);
            }
        }
        terminal.draw(|frame| view::draw(frame, &app, theme))?;
        if !event::poll(Duration::from_millis(200))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if let Some(confirm) = app.confirm.take() {
            if matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                app.message = Some(format!("stopping {} ...", confirm.id));
                terminal.draw(|frame| view::draw(frame, &app, theme))?;
                app.message = Some(
                    match crate::commands::run::stop_in(ctx, &confirm.dir, &confirm.id, false) {
                        Ok(_) => format!("stopped {}", confirm.id),
                        Err(err) => format!("could not stop {}: {err}", confirm.id),
                    },
                );
                // A clear after docker may have written to the terminal.
                terminal.clear()?;
                now.store(true, Ordering::Relaxed);
            } else {
                app.message = Some("left running".to_string());
            }
            continue;
        }
        app.message = None;
        match key.code {
            KeyCode::Char('q') => break Ok(()),
            KeyCode::Esc if app.logs.is_some() => app.logs = None,
            KeyCode::Esc => break Ok(()),
            KeyCode::Up | KeyCode::Char('k') => app.up(),
            KeyCode::Down | KeyCode::Char('j') => app.down(),
            KeyCode::Enter | KeyCode::Char(' ') => app.toggle(),
            KeyCode::Char('r') => now.store(true, Ordering::Relaxed),
            KeyCode::Char('l') if app.logs.is_some() => app.logs = None,
            KeyCode::Char('l') => app.logs = Some(ask_logs(&app, &logs_tx)),
            KeyCode::Char('s') => match app.selected_service() {
                Some((node, service)) => match (&node.dir, &service.model) {
                    (Some(dir), Some(id)) if service.role == "model" => {
                        app.confirm = Some(Confirm {
                            dir: dir.clone(),
                            id: id.clone(),
                            deployment: node.name.clone(),
                        });
                    }
                    _ => {
                        app.message = Some(format!(
                            "only a model stops here; `chap-devops -C {} down` stops {}",
                            node.dir
                                .as_ref()
                                .map(|d| d.display().to_string())
                                .unwrap_or_else(|| node.name.clone()),
                            node.name
                        ));
                    }
                },
                None => app.message = Some("select a model first".to_string()),
            },
            _ => {}
        }
    };
    quit.store(true, Ordering::Relaxed);
    result
}

/// Open the log pane for the selected service, and ask docker for its last
/// lines on a thread that sends them to `tx` when it has them.
fn ask_logs(app: &App, tx: &mpsc::Sender<Logs>) -> Logs {
    let Some(Entry::Service(n, s)) = app.selected_entry() else {
        return Logs {
            title: "select a service to see its log".to_string(),
            lines: Vec::new(),
        };
    };
    let node = &app.nodes[n];
    let service = node.services[s].service.clone();
    let title = format!("{} / {}", node.name, service);
    let (dir, tx, sent_title) = (node.dir.clone(), tx.clone(), title.clone());
    std::thread::spawn(move || {
        let _ = tx.send(Logs {
            title: sent_title,
            lines: log_lines(dir.as_deref(), &service),
        });
    });
    Logs {
        title,
        lines: vec!["asking docker ...".to_string()],
    }
}

/// The last lines of one service's log, colour codes taken out.
fn log_lines(dir: Option<&std::path::Path>, service: &str) -> Vec<String> {
    let text = dir
        .and_then(|dir| crate::project::Project::load(dir).ok())
        .and_then(|project| crate::docker::service_logs(&project, service, LOG_LINES));
    match text {
        Some(text) => String::from_utf8_lossy(&crate::docker::strip_ansi(text.as_bytes()))
            .lines()
            .map(str::to_string)
            .collect(),
        None => vec!["docker has no log for it".to_string()],
    }
}

/// The wall clock as `HH:MM:SS`, local time.
fn clock() -> String {
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    crate::output::local_clock_seconds(unix)
}

#[cfg(test)]
mod tests;
