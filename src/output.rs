use indicatif::{ProgressBar, ProgressStyle};
use std::sync::Mutex;
use tracing::info;

/// Centralized output manager that handles both spinner and flat logging modes
pub struct Output {
    mode: OutputMode,
    spinner: Option<Mutex<ProgressBar>>,
}

enum OutputMode {
    /// Interactive spinner mode (normal and dry-run)
    Interactive,
    /// Flat logging mode (verbose -v)
    Flat,
}

impl Output {
    /// Create new output manager
    ///
    /// # Arguments
    /// * `verbose` - If true, uses flat logging mode; if false, uses interactive spinner mode
    pub fn new(verbose: bool) -> Self {
        let mode = if verbose {
            OutputMode::Flat
        } else {
            OutputMode::Interactive
        };

        let spinner = match mode {
            OutputMode::Interactive => {
                let pb = ProgressBar::new_spinner();
                pb.set_style(
                    ProgressStyle::default_spinner()
                        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                        .template("{spinner:.green} {msg}")
                        .expect("Failed to create spinner template"),
                );
                pb.enable_steady_tick(std::time::Duration::from_millis(80));
                Some(Mutex::new(pb))
            }
            OutputMode::Flat => None,
        };

        Self { mode, spinner }
    }

    /// Log a current/in-progress action (shows in spinner)
    pub fn log_current(&self, message: impl AsRef<str>) {
        match self.mode {
            OutputMode::Interactive => {
                if let Some(ref spinner) = self.spinner {
                    spinner
                        .lock()
                        .unwrap()
                        .set_message(message.as_ref().to_string());
                }
            }
            OutputMode::Flat => {
                info!("{}", message.as_ref());
            }
        }
    }

    /// Log a static message (for banners, summaries, completions, etc.)
    pub fn log_message(&self, message: impl AsRef<str>) {
        match self.mode {
            OutputMode::Interactive => {
                if let Some(ref spinner) = self.spinner {
                    spinner.lock().unwrap().println(message.as_ref());
                }
            }
            OutputMode::Flat => {
                info!("{}", message.as_ref());
            }
        }
    }

    /// Finish and cleanup spinner
    pub fn finish(&self) {
        if let Some(ref spinner) = self.spinner {
            spinner.lock().unwrap().finish_and_clear();
        }
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        if let Some(ref spinner) = self.spinner {
            spinner.lock().unwrap().finish_and_clear();
        }
    }
}
