use std::sync::RwLock;

use indicatif::{ProgressBar, ProgressStyle};
use owo_colors::OwoColorize;
use tracing::info;

pub trait Output: Send + Sync {
    fn log_current(&self, message: &str);

    fn set_substep(&self, message: &str);

    fn start_substep(&self, message: String) -> Substep<'_>;

    fn log_message(&self, message: &str);
    fn log_completed(&self, message: &str);

    fn finish(&self) {}
}

/// Interactive spinner mode implementation
pub struct InteractiveOutput {
    spinner: ProgressBar,
    current_text: RwLock<String>,
    substeps: RwLock<Vec<String>>,
}

impl InteractiveOutput {
    pub fn new() -> Self {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                .template("{spinner:.green} {msg}")
                .expect("Failed to create spinner template"),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(80));
        Self {
            spinner: pb,
            current_text: RwLock::new(String::new()),
            substeps: RwLock::new(Vec::new()),
        }
    }
}

impl Default for InteractiveOutput {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Substep<'a> {
    finish: Box<dyn Fn() + 'a>,
}

impl<'a> Substep<'a> {
    pub fn new(finish: Box<dyn Fn() + 'a>) -> Self {
        Self { finish }
    }
}

impl<'a> Drop for Substep<'a> {
    fn drop(&mut self) {
        (self.finish)();
    }
}

impl Output for InteractiveOutput {
    fn log_current(&self, message: &str) {
        let mut current_text = self.current_text.write().unwrap();
        *current_text = message.to_string();
        self.spinner.set_message(format!("{}...", current_text));
    }

    fn set_substep(&self, message: &str) {
        if message.is_empty() {
            self.spinner
                .set_message(format!("{}...", self.current_text.read().unwrap()));
        } else {
            self.spinner.set_message(format!(
                "{} {}...",
                self.current_text.read().unwrap(),
                format!("({})", message).dimmed()
            ));
        }
    }

    fn start_substep(&self, message: String) -> Substep<'_> {
        let mut substeps = self.substeps.write().unwrap();
        substeps.push(message.clone());
        self.set_substep(&substeps.join(", "));

        Substep::new(Box::new(move || {
            let mut substeps = self.substeps.write().unwrap();
            substeps.retain(|s| s != &message);
            self.set_substep(&substeps.join(", "));
        }))
    }

    fn log_message(&self, message: &str) {
        self.spinner.println(message);
    }

    fn log_completed(&self, message: &str) {
        self.log_message(message);
        self.spinner.set_message(String::new());
    }

    fn finish(&self) {
        self.spinner.finish_and_clear();
    }
}

impl Drop for InteractiveOutput {
    fn drop(&mut self) {
        self.spinner.finish_and_clear();
    }
}

/// Flat logging mode implementation
pub struct FlatOutput {
    current_text: RwLock<String>,
    substeps: RwLock<Vec<String>>,
}

impl FlatOutput {
    pub fn new() -> Self {
        Self {
            current_text: RwLock::new(String::new()),
            substeps: RwLock::new(Vec::new()),
        }
    }
}

impl Default for FlatOutput {
    fn default() -> Self {
        Self::new()
    }
}

impl Output for FlatOutput {
    fn log_current(&self, message: &str) {
        let mut current_text = self.current_text.write().unwrap();
        *current_text = message.to_string();
        info!("{}", message);
    }

    fn set_substep(&self, message: &str) {
        info!(
            "{} {}",
            self.current_text.read().unwrap(),
            format!("({})", message).dimmed()
        );
    }

    fn start_substep(&self, message: String) -> Substep<'_> {
        let mut substeps = self.substeps.write().unwrap();
        substeps.push(message.clone());
        self.set_substep(&substeps.join(", "));

        Substep::new(Box::new(move || {
            let mut substeps = self.substeps.write().unwrap();
            substeps.retain(|s| s != &message);
            self.set_substep(&substeps.join(", "));
        }))
    }

    fn log_message(&self, message: &str) {
        info!("{}", message);
    }

    fn log_completed(&self, message: &str) {
        info!("{}", message);
    }
}

pub struct BufferedOutput {
    current_text: RwLock<String>,
    substeps: RwLock<Vec<String>>,

    buffer: RwLock<String>,
}

impl BufferedOutput {
    pub fn new() -> Self {
        Self {
            current_text: RwLock::new(String::new()),
            substeps: RwLock::new(Vec::new()),
            buffer: RwLock::new(String::new()),
        }
    }

    pub fn get_buffer(&self) -> String {
        self.buffer.read().unwrap().clone()
    }
}

impl Default for BufferedOutput {
    fn default() -> Self {
        Self::new()
    }
}

impl Output for BufferedOutput {
    fn log_current(&self, message: &str) {
        let mut current_text = self.current_text.write().unwrap();
        *current_text = message.to_string();
        let mut buffer = self.buffer.write().unwrap();
        buffer.push_str(&format!("{}...\n", message));
    }

    fn start_substep(&self, message: String) -> Substep<'_> {
        let mut substeps = self.substeps.write().unwrap();
        substeps.push(message.clone());
        let mut buffer = self.buffer.write().unwrap();
        buffer.push_str(&format!(
            "{} {}...\n",
            self.current_text.read().unwrap(),
            format!("({})", message).dimmed()
        ));

        Substep::new(Box::new(move || {
            let mut substeps = self.substeps.write().unwrap();
            substeps.retain(|s| s != &message);
            let mut buffer = self.buffer.write().unwrap();

            if substeps.is_empty() {
                buffer.push_str(&format!("{}...\n", self.current_text.read().unwrap()));
            } else {
                buffer.push_str(&format!(
                    "{} {}...\n",
                    self.current_text.read().unwrap(),
                    format!("({})", substeps.join(", ")).dimmed()
                ));
            }
        }))
    }

    fn set_substep(&self, message: &str) {
        let current_text = self.current_text.read().unwrap();
        let mut buffer = self.buffer.write().unwrap();

        if message.is_empty() {
            buffer.push_str(&format!("{}...\n", current_text));
        } else {
            buffer.push_str(&format!(
                "{} {}...\n",
                current_text,
                format!("({})", message).dimmed()
            ));
        }
    }

    fn log_message(&self, message: &str) {
        let mut buffer = self.buffer.write().unwrap();
        buffer.push_str(&format!("{}...\n", message));
    }

    fn log_completed(&self, message: &str) {
        let mut buffer = self.buffer.write().unwrap();
        buffer.push_str(&format!("{}...\n", message));
    }
}
