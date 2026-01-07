use crate::error::Result;
use std::io::{self, Write};

/// Print a message to stdout
pub fn output(message: &str) -> Result<()> {
    writeln!(io::stdout(), "{}", message)?;
    Ok(())
}

/// Print an error message to stderr
pub fn error(message: &str) -> Result<()> {
    writeln!(io::stderr(), "{}", message)?;
    Ok(())
}
