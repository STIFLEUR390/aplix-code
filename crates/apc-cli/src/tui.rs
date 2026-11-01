//! Full-screen chat view (v1.1): transcript, streaming output, single-line input.
//!
//! Only used when stdin and stdout are terminals; a piped `apc chat` keeps the plain REPL, which is
//! what the CLI tests and any script rely on.

use std::io::IsTerminal;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use apc_agent::Agent;
use apc_core::chat::{ChatEvent, Message, SessionId};
use apc_memory::JsonlStore;
use apc_traits::{Approver, MemoryStore};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Paragraph, Wrap};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

const TICK: Duration = Duration::from_millis(50);
const SPINNER: [&str; 4] = ["⠋", "⠙", "⠹", "⠸"];

/// A shell confirmation the agent is blocked on, answered by the next `y`/`n`.
pub(crate) struct Ask {
    command: String,
    risk: f64,
    reply: Sender<bool>,
}

/// The view's side of the guard: hands the question to the render loop and waits for the keystroke.
pub struct TuiApprover(Sender<Ask>);

impl TuiApprover {
    pub fn channel() -> (Self, Receiver<Ask>) {
        let (tx, rx) = mpsc::channel();
        (Self(tx), rx)
    }
}

impl Approver for TuiApprover {
    fn confirm(&self, command: &str, risk: f64) -> apc_core::Result<bool> {
        let (reply, answer) = mpsc::channel();
        let ask = Ask {
            command: command.to_owned(),
            risk,
            reply,
        };
        self.0
            .send(ask)
            .map_err(|_| apc_core::Error::Tool("the chat view is gone".into()))?;
        answer
            .recv()
            .map_err(|_| apc_core::Error::Tool("the chat view closed before answering".into()))
    }
}

/// What the agent task reports back to the view.
enum Event {
    Delta(String),
    Done(Option<String>),
}

enum Mode {
    Idle,
    Busy(UnboundedReceiver<Event>),
}

/// Height the transcript takes once soft-wrapped at `width`, counted the cheap way.
fn wrapped_height(text: &str, width: u16) -> usize {
    let width = usize::from(width.max(1));
    text.lines()
        .map(|line| line.chars().count().div_ceil(width).max(1))
        .sum::<usize>()
        .max(1)
}

pub fn available() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

struct App {
    session: SessionId,
    transcript: String,
    input: String,
    mode: Mode,
    pending: Option<Ask>,
    tick: usize,
    scroll: u16,
}

impl App {
    fn draw(&self, frame: &mut ratatui::Frame) {
        let [body, footer] =
            Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(frame.area());

        let paragraph = Paragraph::new(self.transcript.as_str())
            .block(Block::bordered().title(format!(" apc — {} ", self.session)))
            .wrap(Wrap { trim: false });

        // Follow the tail of the transcript, minus whatever the user paged back by.
        let inner = Block::bordered().inner(body);
        let offset = (wrapped_height(&self.transcript, inner.width) as u16)
            .saturating_sub(inner.height)
            .saturating_sub(self.scroll);
        frame.render_widget(paragraph.scroll((offset, 0)), body);

        if let Some(ask) = &self.pending {
            frame.render_widget(
                Paragraph::new(format!(
                    " run it? {:.0}% destructive  [y/N]  {}",
                    ask.risk * 100.0,
                    ask.command
                ))
                .style(Style::default().fg(Color::Red)),
                footer,
            );
        } else if matches!(self.mode, Mode::Busy(_)) {
            frame.render_widget(
                Paragraph::new(format!(
                    " {} thinking… (esc to quit)",
                    SPINNER[self.tick % 4]
                ))
                .style(Style::default().fg(Color::DarkGray)),
                footer,
            );
        } else {
            frame.render_widget(Paragraph::new(format!(" › {}▏", self.input)), footer);
        }
    }

    fn note(&mut self, text: &str) {
        if !self.transcript.is_empty() && !self.transcript.ends_with('\n') {
            self.transcript.push('\n');
        }
        self.transcript.push_str(text);
        self.transcript.push('\n');
        self.scroll = 0;
    }

    /// Handles one keystroke. Returns true when the user wants to leave the view.
    fn on_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::{KeyCode, KeyModifiers};

        if let Some(ask) = self.pending.as_mut() {
            let answer = match key.code {
                KeyCode::Char('y' | 'Y') => Some(true),
                KeyCode::Char('n' | 'N') | KeyCode::Esc => Some(false),
                _ => None,
            };
            if let Some(answer) = answer {
                let _ = ask.reply.send(answer);
                self.pending = None;
            }
            return false;
        }

        if key.code == KeyCode::Esc
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            return true;
        }

        if matches!(self.mode, Mode::Idle) {
            match key.code {
                KeyCode::Char(c) => self.input.push(c),
                KeyCode::Backspace => {
                    self.input.pop();
                }
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::PageUp => self.scroll = self.scroll.saturating_add(5),
                KeyCode::PageDown => self.scroll = self.scroll.saturating_sub(5),
                _ => {}
            }
        }
        false
    }
}

pub async fn chat(
    agent: Arc<Agent>,
    session: SessionId,
    messages: Vec<Message>,
    asks: Receiver<Ask>,
) -> Result<()> {
    let mut terminal = ratatui::init();
    let mut app = App {
        session: session.clone(),
        transcript: String::new(),
        input: String::new(),
        mode: Mode::Idle,
        pending: None,
        tick: 0,
        scroll: 0,
    };
    for message in &messages {
        app.note(&format!("{}: {}", message.role.as_str(), message.content));
    }

    loop {
        app.tick += 1;
        // The agent blocks on the guard until this loop answers, so pick the question up here too.
        if app.pending.is_none() {
            if let Ok(ask) = asks.try_recv() {
                app.pending = Some(ask);
            }
        }

        terminal.draw(|frame| app.draw(frame))?;

        while crossterm::event::poll(Duration::ZERO)? {
            let crossterm::event::Event::Key(key) = crossterm::event::read()? else {
                continue;
            };
            if key.kind != crossterm::event::KeyEventKind::Press {
                continue;
            }
            if app.on_key(key) {
                ratatui::restore();
                return Ok(());
            }
            let submitted = key.code == crossterm::event::KeyCode::Enter
                && matches!(app.mode, Mode::Idle)
                && !app.input.trim().is_empty();
            if submitted {
                let prompt = std::mem::take(&mut app.input);
                if matches!(prompt.trim(), "exit" | "quit") {
                    ratatui::restore();
                    return Ok(());
                }
                app.note(&format!("› {prompt}"));
                let (tx, rx) = unbounded_channel();
                app.mode = Mode::Busy(rx);
                tokio::spawn(run_turn(
                    Arc::clone(&agent),
                    session.clone(),
                    Message::user(prompt.trim()),
                    tx,
                ));
            }
        }

        if let Mode::Busy(rx) = &mut app.mode {
            match rx.try_recv() {
                Ok(Event::Delta(delta)) => {
                    let transcript = &mut app.transcript;
                    transcript.push_str(&delta);
                }
                Ok(Event::Done(error)) => {
                    if let Some(error) = error {
                        app.note(&format!("error: {error}"));
                    }
                    app.mode = Mode::Idle;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => app.mode = Mode::Idle,
            }
        }

        tokio::time::sleep(TICK).await;
    }
}

/// One agent turn: persist the prompt, run, persist every message the agent appended.
async fn run_turn(
    agent: Arc<Agent>,
    session: SessionId,
    prompt: Message,
    tx: UnboundedSender<Event>,
) {
    let result = match JsonlStore::new() {
        Ok(store) => {
            let mut messages = match store.load(Some(&session)).await {
                Ok(messages) => messages,
                Err(error) => {
                    let _ = tx.send(Event::Done(Some(error.to_string())));
                    return;
                }
            };
            let before = messages.len();
            messages.push(prompt);
            if let Err(error) = store.append(&session, &messages[before]).await {
                let _ = tx.send(Event::Done(Some(error.to_string())));
                return;
            }
            let sent = tx.clone();
            let outcome = agent
                .run(&mut messages, move |event| {
                    if let ChatEvent::Delta(delta) = event {
                        let _ = sent.send(Event::Delta(delta.clone()));
                    }
                })
                .await;
            for message in &messages[before + 1..] {
                let _ = store.append(&session, message).await;
            }
            outcome
        }
        Err(error) => Err(error),
    };
    let _ = tx.send(Event::Done(result.err().map(|error| error.to_string())));
}
