use std::{env, path::PathBuf};

use clap::{Parser, Subcommand};
use jj_mrs::{
    cli::cli_main,
    commands::{init, submit},
    error::{Error, Result},
    jj::Jujutsu,
    output::{FlatOutput, InteractiveOutput, Output},
    tracing_formatter::PlainFormatter,
};
use tracing::Level;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    cli_main().await
}
