use crate::bookmark::BranchStack;
use crate::config::StackFormat;
use crate::error::Result;
use crate::gitlab::MergeRequest;
use std::collections::HashMap;

/// Stack description management and formatting for MR descriptions
/// Abstraction for different stack visualization formats
pub trait DescriptionFormatter {
    /// Format the stack visualization
    fn format_stack(&self, stack: &StackContext, current_bookmark: &str) -> String;

    /// Start marker for the stack section
    fn start_marker(&self) -> &'static str;

    /// End marker for the stack section
    fn end_marker(&self) -> &'static str;
}

/// Linear list formatter (like jj-stack)
pub struct LinearListFormatter;

impl DescriptionFormatter for LinearListFormatter {
    fn format_stack(&self, stack: &StackContext, current_bookmark: &str) -> String {
        let mut lines = Vec::new();

        // Header
        lines.push(format!(
            "This MR is part of a stack of {} MRs:",
            stack.bookmarks.len()
        ));
        lines.push("".to_string());

        // Bookmarks (no base branch in the list)
        for (idx, bookmark) in stack.bookmarks.iter().enumerate() {
            let num = idx + 1;

            if bookmark.name == current_bookmark {
                // Current bookmark - bold with marker
                lines.push(format!("{}. **{} ← this MR**", num, bookmark.name));
            } else if let Some(iid) = bookmark.mr_iid {
                // Other bookmark with MR - use !{iid} format (GitLab auto-links)
                lines.push(format!("{}. {} - !{}", num, bookmark.name, iid));
            } else {
                // Bookmark without MR yet
                lines.push(format!("{}. {}", num, bookmark.name));
            }
        }

        lines.join("\n")
    }

    fn start_marker(&self) -> &'static str {
        "<!-- start jj-mrs stack -->"
    }

    fn end_marker(&self) -> &'static str {
        "<!-- end jj-mrs stack -->"
    }
}

/// Context for building stack visualizations
pub struct StackContext {
    /// Bookmarks in the stack (ordered from base to tip)
    pub bookmarks: Vec<StackBookmarkInfo>,

    /// Base branch name (e.g., "main", "master")
    pub base_branch: String,
}

/// Information about a bookmark in the stack
pub struct StackBookmarkInfo {
    /// Bookmark name
    pub name: String,

    /// MR IID if it exists
    pub mr_iid: Option<u64>,

    /// MR URL if it exists
    pub mr_url: Option<String>,
}

/// Result of parsing a description
pub struct ParsedDescription {
    /// User-provided content before the stack section
    pub content_before: Option<String>,
    /// User-provided content after the stack section
    pub content_after: Option<String>,
}

/// Manager for parsing and generating MR descriptions
pub struct DescriptionManager {
    formatter: Box<dyn DescriptionFormatter>,
}

impl DescriptionManager {
    /// Create a new description manager with the given formatter
    pub fn new(formatter: Box<dyn DescriptionFormatter>) -> Self {
        Self { formatter }
    }

    /// Parse an existing description and extract user content before and after markers
    pub fn parse_description(&self, description: &str) -> ParsedDescription {
        if description.is_empty() {
            return ParsedDescription {
                content_before: None,
                content_after: None,
            };
        }

        let start_marker = self.formatter.start_marker();
        let end_marker = self.formatter.end_marker();

        // If no stack section markers, entire description is content before
        if !description.contains(start_marker) {
            return ParsedDescription {
                content_before: Some(description.to_string()),
                content_after: None,
            };
        }

        // Find start and end markers
        let start_pos = description.find(start_marker);
        let end_pos = description.find(end_marker);

        match (start_pos, end_pos) {
            (Some(start), Some(end)) if start < end => {
                // Both markers found in correct order
                let before = &description[..start];
                let after = &description[end + end_marker.len()..];

                let content_before = if before.trim().is_empty() {
                    None
                } else {
                    Some(before.trim().to_string())
                };

                let content_after = if after.trim().is_empty() {
                    None
                } else {
                    Some(after.trim().to_string())
                };

                ParsedDescription {
                    content_before,
                    content_after,
                }
            }
            _ => {
                // Malformed markers - treat entire description as content before
                ParsedDescription {
                    content_before: Some(description.to_string()),
                    content_after: None,
                }
            }
        }
    }

    /// Get the start marker for this formatter
    pub fn start_marker(&self) -> &'static str {
        self.formatter.start_marker()
    }

    /// Get the end marker for this formatter
    pub fn end_marker(&self) -> &'static str {
        self.formatter.end_marker()
    }

    /// Generate a new description with stack visualization and user content
    pub fn generate_description(
        &self,
        content_before: Option<&str>,
        content_after: Option<&str>,
        stack_context: &StackContext,
        current_bookmark: &str,
    ) -> String {
        let stack_section = self.formatter.format_stack(stack_context, current_bookmark);
        self.build_description(content_before, content_after, &stack_section)
    }

    /// Build a description with pre-formatted stack content and user content
    pub fn build_description(
        &self,
        content_before: Option<&str>,
        content_after: Option<&str>,
        stack_content: &str,
    ) -> String {
        let start_marker = self.formatter.start_marker();
        let end_marker = self.formatter.end_marker();

        let mut result = String::new();

        // Add content before markers
        if let Some(before) = content_before {
            result.push_str(before);
            result.push_str("\n\n");
        }

        // Add stack section with markers
        result.push_str(start_marker);
        result.push('\n');
        result.push_str(stack_content);
        result.push('\n');
        result.push_str(end_marker);

        // Add content after markers
        if let Some(after) = content_after {
            result.push_str("\n\n");
            result.push_str(after);
        }

        result
    }
}

/// Generate description for a bookmark that may be in multiple stacks
pub fn generate_multi_stack_description(
    bookmark: &str,
    stacks: &[&BranchStack],
    existing_mrs: &HashMap<String, MergeRequest>,
    format: &StackFormat,
    base_branch: &str,
) -> Result<String> {
    if stacks.is_empty() {
        return Ok(String::new());
    }

    // Create formatter based on config
    let formatter: Box<dyn DescriptionFormatter> = match format {
        StackFormat::Linear => Box::new(LinearListFormatter),
    };

    if stacks.len() == 1 {
        // Single stack - use existing format
        let stack = stacks[0];
        let stack_info: Vec<StackBookmarkInfo> = stack
            .bookmarks
            .iter()
            .map(|bm| StackBookmarkInfo {
                name: bm.clone(),
                mr_iid: existing_mrs.get(bm).map(|mr| mr.iid),
                mr_url: existing_mrs.get(bm).map(|mr| mr.web_url.clone()),
            })
            .collect();

        let context = StackContext {
            bookmarks: stack_info,
            base_branch: base_branch.to_string(),
        };

        return Ok(formatter.format_stack(&context, bookmark));
    }

    // Multiple stacks - format each separately
    let mut lines = Vec::new();
    lines.push(format!("This MR is part of {} stacks:", stacks.len()));
    lines.push("".to_string());

    for (idx, stack) in stacks.iter().enumerate() {
        lines.push(format!(
            "Stack {} ({} MRs):",
            idx + 1,
            stack.bookmarks.len()
        ));

        // Build StackContext for this stack
        let stack_info: Vec<StackBookmarkInfo> = stack
            .bookmarks
            .iter()
            .map(|bm| StackBookmarkInfo {
                name: bm.clone(),
                mr_iid: existing_mrs.get(bm).map(|mr| mr.iid),
                mr_url: existing_mrs.get(bm).map(|mr| mr.web_url.clone()),
            })
            .collect();

        let context = StackContext {
            bookmarks: stack_info,
            base_branch: base_branch.to_string(),
        };

        // Format this stack
        let stack_desc = formatter.format_stack(&context, bookmark);

        // Add indented stack lines (skip the header line "This MR is part of...")
        for line in stack_desc.lines().skip(2) {
            lines.push(line.to_string());
        }

        if idx < stacks.len() - 1 {
            lines.push("".to_string()); // Blank line between stacks
        }
    }

    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_description() {
        let manager = DescriptionManager::new(Box::new(LinearListFormatter));
        let parsed = manager.parse_description("");
        assert!(parsed.content_before.is_none());
        assert!(parsed.content_after.is_none());
    }

    #[test]
    fn test_parse_user_content_only() {
        let manager = DescriptionManager::new(Box::new(LinearListFormatter));
        let parsed = manager.parse_description("User's description here");
        assert_eq!(
            parsed.content_before,
            Some("User's description here".to_string())
        );
        assert!(parsed.content_after.is_none());
    }

    #[test]
    fn test_parse_preserves_user_content_after_markers() {
        let desc =
            "<!-- start jj-mrs stack -->\nStack info\n<!-- end jj-mrs stack -->\n\nUser content";
        let manager = DescriptionManager::new(Box::new(LinearListFormatter));
        let parsed = manager.parse_description(desc);
        assert!(parsed.content_before.is_none());
        assert_eq!(parsed.content_after, Some("User content".to_string()));
    }

    #[test]
    fn test_generate_stack_only() {
        let manager = DescriptionManager::new(Box::new(LinearListFormatter));

        let stack = StackContext {
            bookmarks: vec![
                StackBookmarkInfo {
                    name: "bookmark-a".to_string(),
                    mr_iid: Some(100),
                    mr_url: Some("https://gitlab.com/project/-/merge_requests/100".to_string()),
                },
                StackBookmarkInfo {
                    name: "bookmark-b".to_string(),
                    mr_iid: Some(101),
                    mr_url: Some("https://gitlab.com/project/-/merge_requests/101".to_string()),
                },
            ],
            base_branch: "main".to_string(),
        };

        let desc = manager.generate_description(None, None, &stack, "bookmark-b");

        assert!(desc.contains("<!-- start jj-mrs stack -->"));
        assert!(desc.contains("<!-- end jj-mrs stack -->"));
        assert!(desc.contains("bookmark-b ← this MR"));
    }

    #[test]
    fn test_generate_preserves_user_content() {
        let manager = DescriptionManager::new(Box::new(LinearListFormatter));

        let stack = StackContext {
            bookmarks: vec![StackBookmarkInfo {
                name: "bookmark-a".to_string(),
                mr_iid: None,
                mr_url: None,
            }],
            base_branch: "main".to_string(),
        };

        let desc = manager.generate_description(None, Some("User stuff"), &stack, "bookmark-a");
        assert!(desc.ends_with("User stuff"));
    }

    #[test]
    fn test_linear_formatter_current_bookmark_bold() {
        let formatter = LinearListFormatter;

        let stack = StackContext {
            bookmarks: vec![
                StackBookmarkInfo {
                    name: "bookmark-a".to_string(),
                    mr_iid: Some(100),
                    mr_url: Some("https://gitlab.com/mrs/100".to_string()),
                },
                StackBookmarkInfo {
                    name: "bookmark-b".to_string(),
                    mr_iid: None,
                    mr_url: None,
                },
            ],
            base_branch: "main".to_string(),
        };

        let output = formatter.format_stack(&stack, "bookmark-b");
        assert!(output.contains("**bookmark-b ← this MR**"));
        assert!(output.contains("bookmark-a - !100"));
    }

    #[test]
    fn test_linear_formatter_proper_gitlab_format() {
        let formatter = LinearListFormatter;

        let stack = StackContext {
            bookmarks: vec![
                StackBookmarkInfo {
                    name: "feature-1".to_string(),
                    mr_iid: Some(18),
                    mr_url: Some(
                        "https://gitlab.internal.valence.nl/abrenneke/testing/-/merge_requests/18"
                            .to_string(),
                    ),
                },
                StackBookmarkInfo {
                    name: "feature-2".to_string(),
                    mr_iid: None,
                    mr_url: None,
                },
                StackBookmarkInfo {
                    name: "alt-feature".to_string(),
                    mr_iid: Some(19),
                    mr_url: Some(
                        "https://gitlab.internal.valence.nl/abrenneke/testing/-/merge_requests/19"
                            .to_string(),
                    ),
                },
            ],
            base_branch: "main".to_string(),
        };

        let output = formatter.format_stack(&stack, "feature-2");

        // Should NOT include the base branch in the list
        assert!(!output.contains("1. `main`"));

        // Should use "MRs" not "bookmarks" in description
        assert!(output.contains("This MR is part of a stack of 3 MRs"));

        // Should use !{iid} format, not full markdown links
        assert!(output.contains("1. feature-1 - !18"));
        assert!(!output.contains("[feature-1](https://gitlab"));

        // Current MR should be bold with marker
        assert!(output.contains("2. **feature-2 ← this MR**"));

        // Other MR with link should also use !{iid} format
        assert!(output.contains("3. alt-feature - !19"));
        assert!(!output.contains("[alt-feature](https://gitlab"));
    }

    #[test]
    fn test_parse_no_end_marker_malformed() {
        let desc = "<!-- start jj-mrs stack -->\nStack info without end";
        let manager = DescriptionManager::new(Box::new(LinearListFormatter));
        let parsed = manager.parse_description(desc);
        assert_eq!(parsed.content_before, Some(desc.to_string()));
        assert!(parsed.content_after.is_none());
    }

    #[test]
    fn test_round_trip_preserves_user_content() {
        let manager = DescriptionManager::new(Box::new(LinearListFormatter));
        let original =
            "<!-- start jj-mrs stack -->\nOld stack\n<!-- end jj-mrs stack -->\n\nMy notes";
        let parsed = manager.parse_description(original);

        let stack = StackContext {
            bookmarks: vec![StackBookmarkInfo {
                name: "feature".to_string(),
                mr_iid: Some(100),
                mr_url: Some("url".to_string()),
            }],
            base_branch: "main".to_string(),
        };

        let new_desc = manager.generate_description(
            parsed.content_before.as_deref(),
            parsed.content_after.as_deref(),
            &stack,
            "feature",
        );
        assert!(new_desc.contains("My notes"));
    }

    #[test]
    fn test_generate_no_trailing_whitespace_when_no_user_content() {
        let manager = DescriptionManager::new(Box::new(LinearListFormatter));
        let stack = StackContext {
            bookmarks: vec![StackBookmarkInfo {
                name: "f".to_string(),
                mr_iid: Some(1),
                mr_url: Some("u".to_string()),
            }],
            base_branch: "main".to_string(),
        };

        let desc = manager.generate_description(None, None, &stack, "f");
        assert!(desc.ends_with("<!-- end jj-mrs stack -->"));
    }

    #[test]
    fn test_parse_content_before_and_after_markers() {
        let manager = DescriptionManager::new(Box::new(LinearListFormatter));
        let desc = "Content before\n\n<!-- start jj-mrs stack -->\nStack info\n<!-- end jj-mrs stack -->\n\nContent after";
        let parsed = manager.parse_description(desc);

        assert_eq!(parsed.content_before, Some("Content before".to_string()));
        assert_eq!(parsed.content_after, Some("Content after".to_string()));
    }

    #[test]
    fn test_generate_with_content_before_and_after() {
        let manager = DescriptionManager::new(Box::new(LinearListFormatter));
        let stack = StackContext {
            bookmarks: vec![StackBookmarkInfo {
                name: "feature".to_string(),
                mr_iid: Some(100),
                mr_url: Some("url".to_string()),
            }],
            base_branch: "main".to_string(),
        };

        let desc = manager.generate_description(
            Some("Before content"),
            Some("After content"),
            &stack,
            "feature",
        );

        assert!(desc.starts_with("Before content"));
        assert!(desc.contains("<!-- start jj-mrs stack -->"));
        assert!(desc.contains("<!-- end jj-mrs stack -->"));
        assert!(desc.ends_with("After content"));
    }
}
