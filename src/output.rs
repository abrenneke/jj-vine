use std::cell::RefCell;

use indicatif::{ProgressBar, ProgressStyle};
use owo_colors::OwoColorize;
use tracing::info;

pub trait Output {
    fn log_current(&self, message: &str);
    fn set_substep(&self, message: &str);

    fn log_message(&self, message: &str);
    fn log_completed(&self, message: &str);

    fn finish(&self) {}
}

/// Interactive spinner mode implementation
pub struct InteractiveOutput {
    spinner: ProgressBar,
    current_text: RefCell<String>,
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
            current_text: RefCell::new(String::new()),
        }
    }
}

impl Default for InteractiveOutput {
    fn default() -> Self {
        Self::new()
    }
}

impl Output for InteractiveOutput {
    fn log_current(&self, message: &str) {
        self.current_text.replace(message.to_string());
        self.spinner
            .set_message(format!("{}...", self.current_text.borrow().clone()));
    }

    fn set_substep(&self, message: &str) {
        self.spinner.set_message(format!(
            "{} {}...",
            self.current_text.borrow(),
            format!("({})", message).dimmed()
        ));
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
    current_text: RefCell<String>,
}

impl FlatOutput {
    pub fn new() -> Self {
        Self {
            current_text: RefCell::new(String::new()),
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
        self.current_text.replace(message.to_string());
        info!("{}", message);
    }

    fn set_substep(&self, message: &str) {
        info!(
            "{}",
            format!(
                "{} {}",
                self.current_text.borrow(),
                format!("({})", message).dimmed()
            )
        );
    }

    fn log_message(&self, message: &str) {
        info!("{}", message);
    }

    fn log_completed(&self, message: &str) {
        info!("{}", message);
    }
}
