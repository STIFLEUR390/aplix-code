//! Full-screen chat view (v1.1): transcript, streaming output, a status bar and slash commands.
//!
//! Only used when stdin and stdout are terminals; a piped `apc chat` keeps the plain REPL, which is
//! what the CLI tests and any script rely on.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use apc_agent::Agent;
use apc_core::chat::{ChatEvent, Message, SessionId};
use apc_core::config;
use apc_memory::SqliteStore;
use apc_traits::{Approver, MemoryStore};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Paragraph, Wrap};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

const TICK: Duration = Duration::from_millis(50);
const SPINNER: [&str; 4] = ["⠋", "⠙", "⠹", "⠸"];

/// What `/help` lists; the order is the order they are offered in.
const COMMANDS: [(&str, &str); 6] = [
    ("/help", "this list"),
    (
        "/mode",
        "[plan|build] — plan offers read_file, list_dir and the web tools only",
    ),
    (
        "/model",
        "[provider] [model] — switch, the agent is rebuilt",
    ),
    ("/skills", "list the skills in ./skills"),
    ("/skill", "[name|auto] — force a skill for this session"),
    ("/quit", "leave the view"),
];

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

enum Status {
    Idle,
    Busy(UnboundedReceiver<Event>),
}

/// One slash command, already split into words.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    Help,
    Quit,
    /// `None` toggles between the two modes.
    Mode(Option<String>),
    /// No provider reports what is configured; one switches and rebuilds.
    Model {
        provider: Option<String>,
        model: Option<String>,
    },
    Skills,
    /// `None` reports, `auto` clears, anything else forces that skill.
    Skill(Option<String>),
    Unknown(String),
}

/// Parses a line into a command; `None` when it is an ordinary prompt.
fn command(line: &str) -> Option<Command> {
    let text = line.strip_prefix('/')?;
    let mut words = text.split_whitespace();
    let name = words.next().unwrap_or_default();
    let rest: Vec<&str> = words.collect();
    Some(match name {
        "help" | "?" => Command::Help,
        "quit" | "exit" => Command::Quit,
        "mode" => Command::Mode(rest.first().map(|word| (*word).to_owned())),
        "model" => Command::Model {
            provider: rest.first().map(|word| (*word).to_owned()),
            model: rest.get(1).map(|word| (*word).to_owned()),
        },
        "skills" => Command::Skills,
        "skill" => Command::Skill(rest.first().map(|word| (*word).to_owned())),
        other => Command::Unknown(other.to_owned()),
    })
}

/// Command names matching what has been typed, for the hint line and Tab.
fn matches(input: &str) -> Vec<&'static str> {
    if !input.starts_with('/') || input.contains(' ') {
        return Vec::new();
    }
    COMMANDS
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| name.starts_with(input))
        .collect()
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
    config: config::Config,
    approver: Arc<dyn Approver>,
    mode: config::Mode,
    skill: Option<String>,
    agent: Arc<Agent>,
    transcript: String,
    input: String,
    status: Status,
    pending: Option<Ask>,
    tick: usize,
    scroll: u16,
}

impl App {
    /// Rebuilds the agent after a mode or provider change; the tool set is fixed at build time.
    fn rebuild(&mut self) -> Result<()> {
        let agent = super::agent(&self.config, Arc::clone(&self.approver), self.mode)?;
        self.agent = Arc::new(agent);
        Ok(())
    }

    fn draw(&self, frame: &mut ratatui::Frame) {
        let [body, status, footer] = Layout::vertical([
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        let paragraph = Paragraph::new(self.transcript.as_str())
            .block(Block::bordered().title(format!(" apc — {} ", self.session)))
            .wrap(Wrap { trim: false });

        // Follow the tail of the transcript, minus whatever the user paged back by.
        let inner = Block::bordered().inner(body);
        let offset = (wrapped_height(&self.transcript, inner.width) as u16)
            .saturating_sub(inner.height)
            .saturating_sub(self.scroll);
        frame.render_widget(paragraph.scroll((offset, 0)), body);

        let hints = matches(&self.input);
        if !hints.is_empty() {
            frame.render_widget(
                Paragraph::new(format!("  {}", hints.join("   ")))
                    .style(Style::default().fg(Color::DarkGray)),
                status,
            );
        } else {
            frame.render_widget(
                Paragraph::new(format!(
                    " {} │ {}/{} │ skill: {} │ /help",
                    self.mode,
                    self.config.provider.name,
                    self.config.provider.model,
                    self.skill.as_deref().unwrap_or("auto"),
                ))
                .style(Style::default().fg(Color::DarkGray)),
                status,
            );
        }

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
        } else if matches!(self.status, Status::Busy(_)) {
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

    /// Runs a slash command. Returns true when the view should close.
    fn run_command(&mut self, line: &str, root: &Path) -> bool {
        match command(line) {
            Some(Command::Help) => {
                let text = COMMANDS
                    .iter()
                    .map(|(name, about)| format!("  {name:<8} {about}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                self.note(&text);
            }
            Some(Command::Quit) => return true,
            Some(Command::Mode(None)) => {
                self.mode = match self.mode {
                    config::Mode::Plan => config::Mode::Build,
                    config::Mode::Build => config::Mode::Plan,
                };
                self.report_rebuild();
            }
            Some(Command::Mode(Some(word))) => match config::Mode::parse(&word) {
                Some(mode) => {
                    self.mode = mode;
                    self.report_rebuild();
                }
                None => self.note(&format!("no such mode: {word} (plan or build)")),
            },
            Some(Command::Model {
                provider: None,
                model: None,
            }) => self.note(&format!(
                " {} on {} — apc_models knows: {}",
                self.config.provider.model,
                self.config.provider.base_url.as_deref().unwrap_or("-"),
                apc_models::names().join(", "),
            )),
            Some(Command::Model { provider, model }) => {
                let Some(provider) = provider else {
                    return false;
                };
                let before = self.config.clone();
                self.config.use_provider(&provider, model.as_deref());
                if let Err(error) = self.rebuild() {
                    self.config = before;
                    let _ = self.rebuild();
                    self.note(&format!("error: {error}"));
                } else {
                    self.note(&format!(
                        "model is now {}/{}",
                        self.config.provider.name, self.config.provider.model
                    ));
                }
            }
            Some(Command::Skills) => {
                let names = super::skills(root);
                if names.is_empty() {
                    self.note("no skills in ./skills");
                } else {
                    self.note(&format!("skills: {}", names.join(", ")));
                }
            }
            Some(Command::Skill(None)) => self.note(&format!(
                "skill: {} (say /skill <name>)",
                self.skill.as_deref().unwrap_or("auto")
            )),
            Some(Command::Skill(Some(word))) if matches!(word.as_str(), "auto" | "-") => {
                self.skill = None;
                self.note("skills chosen from the prompt again");
            }
            Some(Command::Skill(Some(name))) => {
                if super::skills(root).contains(&name) {
                    self.skill = Some(name.clone());
                    self.note(&format!("every turn now starts with skills/{name}.md"));
                } else {
                    self.note(&format!("no such skill: {name} (see /skills)"));
                }
            }
            Some(Command::Unknown(name)) => self.note(&format!("unknown command /{name} — /help")),
            None => {}
        }
        false
    }

    fn report_rebuild(&mut self) {
        match self.rebuild() {
            Ok(()) => {
                let mode = self.mode;
                self.note(&format!("mode: {mode}"));
            }
            Err(error) => self.note(&format!("error: {error}")),
        }
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

        if matches!(self.status, Status::Idle) {
            match key.code {
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.input.clear();
                }
                KeyCode::Char(c) => self.input.push(c),
                KeyCode::Backspace => {
                    self.input.pop();
                }
                KeyCode::Tab => {
                    // Completes to the first command the prefix still matches.
                    if let Some(name) = matches(&self.input).first() {
                        self.input = format!("{name} ");
                    }
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
    config: config::Config,
    approver: Arc<dyn Approver>,
    session: SessionId,
    messages: Vec<Message>,
    asks: Receiver<Ask>,
    root: PathBuf,
) -> Result<()> {
    let mut terminal = ratatui::init();
    let mut app = App {
        session: session.clone(),
        agent: Arc::new(super::agent(
            &config,
            Arc::clone(&approver),
            config::Mode::Build,
        )?),
        config,
        approver,
        mode: config::Mode::Build,
        skill: None,
        transcript: String::new(),
        input: String::new(),
        status: Status::Idle,
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
                && matches!(app.status, Status::Idle)
                && !app.input.trim().is_empty();
            if submitted {
                let prompt = std::mem::take(&mut app.input);
                let text = prompt.trim();
                if matches!(text, "exit" | "quit") {
                    ratatui::restore();
                    return Ok(());
                }
                app.note(&format!("› {text}"));
                if text.starts_with('/') {
                    if app.run_command(text, &root) {
                        ratatui::restore();
                        return Ok(());
                    }
                    continue;
                }
                let (tx, rx) = unbounded_channel();
                app.status = Status::Busy(rx);
                tokio::spawn(run_turn(
                    Arc::clone(&app.agent),
                    session.clone(),
                    Message::user(text),
                    tx,
                    root.clone(),
                    app.skill.clone(),
                ));
            }
        }

        if let Status::Busy(rx) = &mut app.status {
            match rx.try_recv() {
                Ok(Event::Delta(delta)) => {
                    let transcript = &mut app.transcript;
                    transcript.push_str(&delta);
                }
                Ok(Event::Done(error)) => {
                    if let Some(error) = error {
                        app.note(&format!("error: {error}"));
                    }
                    app.status = Status::Idle;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => app.status = Status::Idle,
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
    root: PathBuf,
    skill: Option<String>,
) {
    let result = match SqliteStore::new() {
        Ok(store) => {
            let mut messages = match store.load(Some(&session)).await {
                Ok(messages) => messages,
                Err(error) => {
                    let _ = tx.send(Event::Done(Some(error.to_string())));
                    return;
                }
            };
            match skill.as_deref() {
                Some(name) => super::apply_skill_file(&mut messages, &root, name),
                None => super::apply_skill(&mut messages, &root, &prompt.content),
            }
            super::apply_memory(&store, &mut messages, &session, &prompt.content).await;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_prompt_is_not_a_command() {
        assert_eq!(command("explique ce fichier"), None);
        assert_eq!(command(""), None);
    }

    #[test]
    fn commands_take_their_arguments() {
        assert_eq!(command("/help"), Some(Command::Help));
        assert_eq!(command("/mode"), Some(Command::Mode(None)));
        assert_eq!(
            command("/mode plan"),
            Some(Command::Mode(Some("plan".to_owned())))
        );
        assert_eq!(
            command("/model ollama llama3"),
            Some(Command::Model {
                provider: Some("ollama".to_owned()),
                model: Some("llama3".to_owned()),
            })
        );
        assert_eq!(command("/skill"), Some(Command::Skill(None)));
        assert_eq!(command("/nope"), Some(Command::Unknown("nope".to_owned())));
    }

    #[test]
    fn the_hint_line_only_matches_command_words() {
        assert_eq!(
            matches("/"),
            vec!["/help", "/mode", "/model", "/skills", "/skill", "/quit"]
        );
        assert_eq!(matches("/mod"), vec!["/mode", "/model"]);
        assert!(matches("/mode plan").is_empty());
        assert!(matches("bonjour").is_empty());
    }
}
