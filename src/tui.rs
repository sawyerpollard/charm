//! The management console shown on `ssh charm@<server>` (and `charm tui`).
//!
//! A second frontend over the same core as the CLI (`app`): it renders the data
//! `app::summaries`/`app::status` return and calls the same actions.

use anyhow::{bail, Result};
use std::io::IsTerminal;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table, TableState};
use ratatui::{DefaultTerminal, Frame};

use crate::app::{self, Health};

struct AppRow {
    name: String,
    kind: &'static str,
    health: Health,
}

struct Ui {
    rows: Vec<AppRow>,
    state: TableState,
    message: String,
    /// In-flight action; its result message arrives over this channel.
    pending: Option<Receiver<String>>,
    /// When set, a modal detail panel for one app is shown.
    detail: Option<app::AppStatus>,
}

pub fn run() -> Result<()> {
    if !std::io::stdout().is_terminal() {
        bail!("charm tui needs a terminal (run `ssh charm@<server>`, or `charm tui` in a terminal)");
    }
    let mut ui = Ui {
        rows: Vec::new(),
        state: TableState::default(),
        message: String::new(),
        pending: None,
        detail: None,
    };
    ui.refresh();

    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut ui);
    ratatui::restore();
    result
}

fn event_loop(terminal: &mut DefaultTerminal, ui: &mut Ui) -> Result<()> {
    loop {
        // Collect a finished background action without blocking.
        if let Some(rx) = &ui.pending {
            if let Ok(msg) = rx.try_recv() {
                ui.message = msg;
                ui.pending = None;
                ui.refresh();
            }
        }

        terminal.draw(|f| draw(f, ui))?;
        if !event::poll(Duration::from_millis(150))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        // Ctrl-C / Ctrl-D arrive as key events in raw mode, not signals.
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('d'))
        {
            return Ok(());
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
            KeyCode::Char('j') | KeyCode::Down => ui.step(1),
            KeyCode::Char('k') | KeyCode::Up => ui.step(-1),
            KeyCode::Char('r') => {
                ui.refresh();
                ui.message = "refreshed".into();
            }
            KeyCode::Char('s') => ui.act("stopped", app::stop),
            KeyCode::Char('g') => ui.act("started", app::start),
            KeyCode::Char('x') => ui.act("restarted", app::restart),
            _ => {}
        }
    }
}

impl Ui {
    fn refresh(&mut self) {
        self.rows = match app::summaries() {
            Ok(apps) => apps
                .into_iter()
                .map(|a| {
                    let health = app::status(&a.name)
                        .map(|s| s.health())
                        .unwrap_or(Health::Missing);
                    AppRow {
                        name: a.name,
                        kind: a.kind,
                        health,
                    }
                })
                .collect(),
            Err(e) => {
                self.message = e.to_string();
                Vec::new()
            }
        };
        if self.rows.is_empty() {
            self.state.select(None);
        } else {
            let sel = self.state.selected().unwrap_or(0).min(self.rows.len() - 1);
            self.state.select(Some(sel));
        }
        self.update_detail();
    }

    fn step(&mut self, delta: isize) {
        let n = self.rows.len();
        if n == 0 {
            return;
        }
        let cur = self.state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(n as isize) as usize;
        self.state.select(Some(next));
        self.update_detail();
    }

    fn act(&mut self, verb: &'static str, f: fn(&str) -> Result<()>) {
        if self.pending.is_some() {
            return; // one action at a time
        }
        let Some(name) = self.state.selected().and_then(|i| self.rows.get(i)).map(|r| r.name.clone())
        else {
            return;
        };
        // Run off-thread so a slow `docker stop` doesn't freeze the UI.
        self.message = format!("{verb} {name}…");
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        thread::spawn(move || {
            let msg = match f(&name) {
                Ok(()) => format!("{verb} {name}: done"),
                Err(e) => format!("{name}: {e}"),
            };
            let _ = tx.send(msg);
        });
    }

    /// Recompute the selected app's detail (called on navigation/refresh).
    fn update_detail(&mut self) {
        self.detail = self
            .state
            .selected()
            .and_then(|i| self.rows.get(i))
            .and_then(|row| app::status(&row.name).ok());
    }
}

fn draw(f: &mut Frame, ui: &mut Ui) {
    let rows = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(f.area());
    let panes =
        Layout::horizontal([Constraint::Min(24), Constraint::Length(42)]).split(rows[0]);

    // Left: app table.
    let header = Row::new(["APP", "KIND", "HEALTH"]).style(Style::new().add_modifier(Modifier::BOLD));
    let table_rows = ui.rows.iter().map(|r| {
        let (label, color) = health_label(&r.health);
        Row::new([r.name.clone(), r.kind.to_string(), label.to_string()])
            .style(Style::new().fg(color))
    });
    let widths = [
        Constraint::Min(12),
        Constraint::Length(8),
        Constraint::Length(11),
    ];
    let table = Table::new(table_rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(" apps "))
        .row_highlight_style(Style::new().reversed())
        .highlight_symbol("> ");
    f.render_stateful_widget(table, panes[0], &mut ui.state);

    // Right: detail of the selected app.
    render_detail(f, panes[1], ui.detail.as_ref());

    // Bottom: message + help.
    f.render_widget(Paragraph::new(ui.message.clone()), rows[1]);
    f.render_widget(
        Paragraph::new("j/k move · s stop · g start · x restart · r refresh · q quit").dim(),
        rows[2],
    );
}

fn render_detail(f: &mut Frame, area: Rect, detail: Option<&app::AppStatus>) {
    let Some(s) = detail else {
        let block = Block::default().borders(Borders::ALL).title(" detail ");
        f.render_widget(Paragraph::new("").block(block), area);
        return;
    };

    let mut lines = vec![
        field_line("kind", s.kind.to_string(), None),
        field_line("url", format!("https://{}", s.host), None),
        field_line("upstream", format!("{}:{}", s.ip, s.port), None),
    ];
    if let Some(img) = &s.image {
        lines.push(field_line("image", img.clone(), None));
    }
    lines.push(field_line(
        "container",
        s.container_state.clone(),
        Some(state_color(&s.container_state)),
    ));
    let (route_txt, route_col) = if s.routed {
        ("published", Color::Green)
    } else {
        ("missing", Color::Red)
    };
    lines.push(field_line("route", route_txt.into(), Some(route_col)));
    lines.push(Line::from(""));
    let (verdict, vcol) = health_label(&s.health());
    lines.push(field_line("status", verdict.to_string(), Some(vcol)));

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", s.name));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn field_line(label: &str, value: String, color: Option<Color>) -> Line<'static> {
    let value = match color {
        Some(c) => Span::styled(value, Style::new().fg(c)),
        None => Span::raw(value),
    };
    Line::from(vec![Span::raw(format!("  {label:<10} ")), value])
}

fn state_color(s: &str) -> Color {
    match s {
        "running" => Color::Green,
        "missing" => Color::Red,
        _ => Color::Yellow,
    }
}

fn health_label(h: &Health) -> (&'static str, Color) {
    match h {
        Health::Healthy => ("healthy", Color::Green),
        Health::NotRouted => ("not routed", Color::Yellow),
        Health::Stopped => ("stopped", Color::Yellow),
        Health::Missing => ("missing", Color::Red),
    }
}
