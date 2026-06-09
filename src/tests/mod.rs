#![expect(clippy::mod_module_files, reason = "want this under tests folder")]
#![expect(clippy::panic_in_result_fn, reason = "tests")]
mod azure;
mod edge_cases;
mod forgejo;
mod github;
mod gitlab;
mod submit;
mod test_helpers;

pub use test_helpers::*;
