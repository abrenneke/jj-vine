#![allow(async_fn_in_trait, reason = "need it")]
#![feature(try_trait_v2)]
#![feature(try_trait_v2_residual)]
#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, reason = "TODO")]
#![allow(
    clippy::unused_async_trait_impl,
    reason = "It's a bit buggy with ? right now"
)]
#![warn(clippy::std_instead_of_core)]
#![warn(clippy::arithmetic_side_effects)]
#![warn(clippy::allow_attributes_without_reason)]
#![warn(clippy::allow_attributes)]
#![warn(clippy::str_to_string)]
#![warn(clippy::redundant_test_prefix)]
#![warn(clippy::doc_paragraphs_missing_punctuation)]
#![warn(clippy::unused_trait_names)]
#![warn(clippy::string_slice)]
#![warn(clippy::missing_assert_message)]
#![warn(clippy::multiple_inherent_impl)]
#![warn(clippy::inline_trait_bounds)]
#![warn(clippy::string_add)]
#![warn(clippy::if_then_some_else_none)]
#![warn(clippy::create_dir)]
#![warn(clippy::module_name_repetitions)]
#![warn(clippy::mod_module_files)]
#![warn(clippy::renamed_function_params)]
#![warn(clippy::partial_pub_fields)]
#![warn(clippy::print_stdout)]
#![warn(clippy::print_stderr)]
#![warn(clippy::default_numeric_fallback)]
#![warn(clippy::unseparated_literal_suffix)]
#![warn(clippy::assigning_clones)]
#![warn(clippy::get_unwrap)]
#![warn(clippy::semicolon_outside_block)]
#![warn(clippy::semicolon_if_nothing_returned)]
#![warn(clippy::iter_over_hash_type)]

// Uncomment to see non-explicitly-enabled lints
// #![warn(clippy::restriction)]
// #![allow(clippy::inline_modules)]
// #![allow(clippy::missing_inline_in_public_items)]
// #![allow(clippy::single_char_lifetime_names)]
// #![allow(clippy::implicit_return)]
// #![allow(clippy::unwrap_used)]
// #![allow(clippy::expect_used)]
// #![allow(clippy::unwrap_in_result)]
// #![allow(clippy::impl_trait_in_params)]
// #![allow(clippy::shadow_reuse)]
// #![allow(clippy::pattern_type_mismatch)]
// #![allow(clippy::exhaustive_enums)]
// #![allow(clippy::arbitrary_source_item_ordering)]
// #![allow(clippy::absolute_paths)]
// #![allow(clippy::wildcard_enum_match_arm)]
// #![allow(clippy::indexing_slicing)]
// #![allow(clippy::missing_docs_in_private_items)]
// #![allow(clippy::question_mark_used)]
// #![allow(clippy::missing_trait_methods)]
// #![allow(clippy::shadow_unrelated)]
// #![allow(clippy::std_instead_of_alloc)]
// #![allow(clippy::exhaustive_structs)]
// #![allow(clippy::min_ident_chars)]
// #![allow(clippy::deref_by_slicing)]
// #![allow(clippy::panic)]
// #![allow(clippy::self_named_module_files)]
// #![allow(clippy::pub_use)]
// #![allow(clippy::pub_with_shorthand)]
// #![allow(clippy::shadow_same)]
// #![allow(clippy::non_ascii_literal)]
// #![allow(clippy::unreachable)]
// #![allow(clippy::separated_literal_suffix)]
// #![allow(clippy::semicolon_inside_block)]

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
