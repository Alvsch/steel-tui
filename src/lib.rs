//! `SteelTui` application made using ratatui

use crate::logger::LOGGER;
use anyhow::Context;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event,
    KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind,
};
use ratatui::crossterm::{ExecutableCommand, event};
use ratatui::layout::Constraint;
use ratatui::prelude::*;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use steel::SteelServer;
use steel_core::server::Server;
use tokio::select;
use tokio::sync::{Notify, mpsc};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{error, info};
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;
use tui_scrollview::{ScrollView, ScrollViewState, ScrollbarVisibility};

static REDRAW: Notify = Notify::const_new();

pub(crate) mod logger;

#[cfg(feature = "plugin")]
mod plugin;

pub use logger::TuiLoggerWriter;
use steel_core::command::sender::CommandSender;
use steel_host::register_default_events;

#[derive(Debug)]
enum AppEvent {
    UiEvent(Event),
}

/// The main application struct
pub struct SteelApp {
    server: Arc<Server>,
    server_token: CancellationToken,
    event_rx: mpsc::Receiver<AppEvent>,
    input: Input,
    scroll_view_state: ScrollViewState,
    scroll_bottom: bool,
    cursor_position: Position,
    token: CancellationToken,
}

impl SteelApp {
    /// Creates a new steel app and spawns an event thread
    ///
    /// # Panics
    /// Panics if the inner thread fails to poll or read events from the terminal
    #[must_use]
    pub fn new(
        server: Arc<Server>,
        token: CancellationToken,
        server_token: CancellationToken,
    ) -> Self {
        let (tx, rx) = mpsc::channel(1);
        let event_token = token.child_token();
        thread::spawn(move || {
            while !event_token.is_cancelled() {
                if event::poll(Duration::from_millis(100)).expect("failed to poll event") {
                    let event = event::read().expect("failed to read event");
                    tx.blocking_send(AppEvent::UiEvent(event))
                        .expect("failed to send");
                }
            }
        });

        Self {
            server,
            server_token,
            event_rx: rx,
            input: Input::new(String::new()),
            scroll_view_state: ScrollViewState::new(),
            scroll_bottom: true,
            cursor_position: Position::default(),
            token,
        }
    }

    fn draw(&mut self, terminal: &mut DefaultTerminal) -> anyhow::Result<()> {
        terminal.draw(|frame| {
            frame.render_widget(&mut *self, frame.area());
            frame.set_cursor_position(self.cursor_position);
        })?;
        Ok(())
    }

    fn submit_message(&mut self) {
        let command = self.input.value_and_reset();
        if command.is_empty() || self.server_token.is_cancelled() {
            return;
        }
        LOGGER.lock().push(Text::raw(format!("> {command}")));
        self.server.command_dispatcher.read().handle_command(
            CommandSender::Console,
            command,
            &self.server,
        );
    }

    fn handle_key(&mut self, event: KeyEvent) {
        if !event.is_press() {
            return;
        }

        if event.code == KeyCode::Char('c') && event.modifiers.contains(KeyModifiers::CONTROL) {
            if self.server_token.is_cancelled() {
                self.token.cancel();
            } else {
                self.server_token.cancel();
            }
        }

        match event.code {
            KeyCode::Enter => self.submit_message(),
            KeyCode::Up => {
                self.scroll_up();
            }
            KeyCode::Down if event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_bottom = true;
            }
            KeyCode::Down => {
                self.scroll_view_state.scroll_down();
            }
            _ => {
                self.input.handle_event(&Event::Key(event));
            }
        }
    }

    const fn scroll_up(&mut self) {
        self.scroll_bottom = false;
        self.scroll_view_state.scroll_up();
    }

    const fn handle_mouse(&mut self, event: MouseEvent) {
        match event.kind {
            MouseEventKind::ScrollDown if event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_bottom = true;
            }
            MouseEventKind::ScrollDown => self.scroll_view_state.scroll_down(),
            MouseEventKind::ScrollUp => self.scroll_up(),
            _ => (),
        }
    }

    /// Starts the steel server
    pub async fn start_server(mut steel_server: SteelServer) {
        let server = steel_server.server.clone();
        let task_tracker = TaskTracker::new();

        #[cfg(feature = "plugin")]
        match plugin::init("plugins").await {
            Ok((mut manager, registry)) => {
                use steel_plugin_sdk::event::PlayerJoinEvent;

                register_default_events(&registry).await;
                manager.enable_all().await;

                let mut event = PlayerJoinEvent {
                    cancelled: false,
                    player: uuid::Uuid::new_v4(),
                };
                registry.call_event(&mut manager, &mut event).await;

                info!("modified: {event:#?}");
            }
            Err(err) => {
                error!("Failed to initialize the plugin system: {err}");
            }
        }

        steel_server.start(task_tracker.clone()).await;
        info!("Waiting for pending tasks...");

        task_tracker.close();
        task_tracker.wait().await;

        for world in &server.worlds {
            world.chunk_map.task_tracker.close();
            world.chunk_map.task_tracker.wait().await;
        }

        // Save all dirty chunks before shutdown
        info!("Saving world data...");
        let mut total_saved = 0;
        for world in &server.worlds {
            world.cleanup(&mut total_saved).await;
        }
        info!("Saved {total_saved} chunks");

        // Save all player data before shutdown
        info!("Saving player data...");
        let mut players_to_save = Vec::new();
        for world in &server.worlds {
            world.players.iter_players(|_, player| {
                players_to_save.push(player.clone());
                true
            });
        }
        match server.player_data_storage.save_all(&players_to_save).await {
            Ok(count) => info!("Saved {count} players"),
            Err(e) => error!("Failed to save player data: {e}"),
        }

        info!("Server stopped");
        LOGGER.lock().push(Text::raw(""));
        LOGGER
            .lock()
            .push("Press Ctrl+C again to exit.".white().bold().into());
    }

    /// Starts the steel tui application
    pub async fn run(&mut self, mut terminal: DefaultTerminal) -> anyhow::Result<()> {
        terminal
            .backend_mut()
            .execute(EnableMouseCapture)
            .context("failed to enable mouse capture")?;
        terminal
            .backend_mut()
            .execute(EnableBracketedPaste)
            .context("failed to enable bracketed paste")?;

        while !self.token.is_cancelled() {
            self.draw(&mut terminal)?;

            let event = select! {
                biased;
                event = self.event_rx.recv() => {
                    if let Some(event) = event { event } else {
                        self.token.cancel();
                        break;
                    }
                }
                () = REDRAW.notified() => {
                    continue;
                }
            };

            match event {
                AppEvent::UiEvent(Event::Key(event)) => self.handle_key(event),
                AppEvent::UiEvent(Event::Mouse(event)) => self.handle_mouse(event),
                AppEvent::UiEvent(Event::Paste(paste)) => {
                    let mut value = self.input.value_and_reset();
                    value.push_str(&paste);

                    replace_with::replace_with(
                        &mut self.input,
                        || Input::new(String::new()),
                        |input| input.with_value(value),
                    );
                }
                AppEvent::UiEvent(_) => (),
            }
        }

        terminal
            .backend_mut()
            .execute(DisableBracketedPaste)
            .context("failed to disable bracketed paste")?;
        terminal
            .backend_mut()
            .execute(DisableMouseCapture)
            .context("failed to disable bracketed paste")?;
        Ok(())
    }
}

impl Widget for &mut SteelApp {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let [text_area, input_area] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);

        let lock = LOGGER.lock();
        let text = &lock.text;

        let content_size = Size::new(text_area.width - 1, text.lines.len() as u16);
        let mut scroll_view = ScrollView::new(content_size)
            .horizontal_scrollbar_visibility(ScrollbarVisibility::Never);

        if self.scroll_view_state.offset().y + text_area.height > content_size.height {
            self.scroll_bottom = true;
        }

        if self.scroll_bottom {
            self.scroll_view_state.scroll_to_bottom();
        }

        self.cursor_position = Position {
            x: self.input.cursor() as u16 + 2,
            y: input_area.y,
        };

        scroll_view.render_widget(text, scroll_view.area());
        scroll_view.render(text_area, buf, &mut self.scroll_view_state);
        Span::raw(format!("> {}", self.input.value())).render(input_area, buf);
    }
}
