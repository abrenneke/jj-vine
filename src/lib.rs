#![allow(async_fn_in_trait)]
#![feature(try_trait_v2)]
#![feature(try_trait_v2_residual)]
#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, reason = "TODO")]
#![allow(
    clippy::unused_async_trait_impl,
    reason = "It's a bit buggy with ? right now"
)]

pub mod bookmark;
pub mod cli;
pub mod commands;
pub mod config;
pub mod description;
pub mod error;
pub mod forge;
pub mod jj;
pub mod output;
pub mod submit;
pub mod title;
pub mod tracing_formatter;

#[cfg(test)]
mod tests;
pub mod utils;
