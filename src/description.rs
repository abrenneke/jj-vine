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
            "This MR is part of a stack of {} bookmarks:",
            stack.bookmarks.len() + 1
        ));
        lines.push("".to_string());

        // Base branch (always first)
        lines.push(format!("1. `{}`", stack.base_branch));

        // Bookmarks
        for (idx, bookmark) in stack.bookmarks.iter().enumerate() {
            let num = idx + 2; // Start from 2 (after base branch)

            if bookmark.name == current_bookmark {
                // Current bookmark - bold with marker
                lines.push(format!("{}. **{} ← this MR**", num, bookmark.name));
            } else if let Some(url) = &bookmark.mr_url {
                // Other bookmark with MR - link
                if let Some(iid) = bookmark.mr_iid {
                    lines.push(format!(
                        "{}. [{}]({}) - MR !{}",
                        num, bookmark.name, url, iid
                    ));
                } else {
                    lines.push(format!("{}. [{}]({})", num, bookmark.name, url));
                }
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
    /// User-provided content (everything outside the stack section)
    pub user_content: Option<String>,
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

    /// Parse an existing description and extract user content
    pub fn parse_description(&self, description: &str) -> ParsedDescription {
        if description.is_empty() {
            return ParsedDescription { user_content: None };
        }

        let start_marker = self.formatter.start_marker();
        let end_marker = self.formatter.end_marker();

        // If no stack section markers, entire description is user content
        if !description.contains(start_marker) {
            return ParsedDescription {
                user_content: Some(description.to_string()),
            };
        }

        // Find the end marker and extract content after it
        if let Some(end_pos) = description.find(end_marker) {
            let after_marker = &description[end_pos + end_marker.len()..];
            let trimmed = after_marker.trim();

            let user_content = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };

            return ParsedDescription { user_content };
        }

        // If end marker not found, malformed description - treat entire thing as user content
        ParsedDescription {
            user_content: Some(description.to_string()),
        }
    }

    /// Generate a new description with stack visualization and user content
    pub fn generate_description(
        &self,
        user_content: Option<&str>,
        stack_context: &StackContext,
        current_bookmark: &str,
    ) -> String {
        let start_marker = self.formatter.start_marker();
        let end_marker = self.formatter.end_marker();
        let stack_section = self.formatter.format_stack(stack_context, current_bookmark);

        let mut result = format!("{}\n{}\n{}", start_marker, stack_section, end_marker);

        if let Some(content) = user_content {
            result.push_str("\n\n");
            result.push_str(content);
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_description() {
        let manager = DescriptionManager::new(Box::new(LinearListFormatter));
        let parsed = manager.parse_description("");
        assert!(parsed.user_content.is_none());
    }

    #[test]
    fn test_parse_user_content_only() {
        let manager = DescriptionManager::new(Box::new(LinearListFormatter));
        let parsed = manager.parse_description("User's description here");
        assert_eq!(
            parsed.user_content,
            Some("User's description here".to_string())
        );
    }

    #[test]
    fn test_parse_preserves_user_content_after_markers() {
        let desc =
            "<!-- start jj-mrs stack -->\nStack info\n<!-- end jj-mrs stack -->\n\nUser content";
        let manager = DescriptionManager::new(Box::new(LinearListFormatter));
        let parsed = manager.parse_description(desc);
        assert_eq!(parsed.user_content, Some("User content".to_string()));
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

        let desc = manager.generate_description(None, &stack, "bookmark-b");

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

        let desc = manager.generate_description(Some("User stuff"), &stack, "bookmark-a");
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
        assert!(output.contains("[bookmark-a](https://gitlab.com/mrs/100) - MR !100"));
    }
}
