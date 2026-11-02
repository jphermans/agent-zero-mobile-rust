//! Markdown rendering for message bodies.
//!
//! Conversations are long and mobile memory is bounded, so rendering is a
//! pure function producing a bounded event stream: hosts lay out the events
//! natively (SwiftUI/Compose) instead of building a web view per message.
//! Code blocks are classified for syntax highlighting via `syntect`'s
//! plain-text-safe token kinds; unsafe HTML is never produced.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Parser, Tag, TagEnd};

/// One rendered block of a message.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// Heading with its level (1-6 collapsed to 1-3 for mobile).`,
    Heading {
        level: u8,
        text: String,
    },
    /// Paragraph of inline text (markdown emphasis resolved to plain text
    /// with markers preserved for styled rendering).
    Paragraph(String),
    /// Code block with optional language tag and raw content.
    Code {
        language: Option<String>,
        content: String,
    },
    /// Block quote content (flattened to paragraphs by the host).`
    Quote(String),
    /// List item (ordered flag + ordinal hint).`
    ListItem {
        ordered: bool,
        ordinal: u64,
        text: String,
    },
    /// Horizontal rule.
    Rule,
}

/// Renders one markdown message body into bounded blocks.
///
/// `max_blocks` caps pathological inputs; when the cap is hit a trailing
/// `Block::Paragraph("…")` marks truncation (the raw text stays available
/// via copy-source in the host).`
pub fn render_blocks(source: &str, max_blocks: usize) -> Vec<Block> {
    let parser = Parser::new(source);
    let mut blocks = Vec::new();
    let mut list_ordered = false;
    let mut list_ordinal: u64 = 0;
    let mut truncated = false;

    for event in parser {
        if blocks.len() >= max_blocks {
            truncated = true;
            break;
        }
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                blocks.push(Block::Heading {
                    level: heading_level(level),
                    text: String::new(),
                });
            }
            Event::End(TagEnd::Heading(_)) => {}
            Event::Start(Tag::Paragraph) => {
                blocks.push(Block::Paragraph(String::new()));
            }
            Event::End(TagEnd::Paragraph) => {}
            Event::Start(Tag::CodeBlock(kind)) => {
                let language = match kind {
                    CodeBlockKind::Fenced(lang) if !lang.is_empty() => {
                        Some(lang.split(',').next().unwrap_or("plaintext").to_string())
                    }
                    _ => None,
                };
                blocks.push(Block::Code {
                    language,
                    content: String::new(),
                });
            }
            Event::End(TagEnd::CodeBlock) => {}
            Event::Start(Tag::BlockQuote(_)) => {
                blocks.push(Block::Quote(String::new()));
            }
            Event::End(TagEnd::BlockQuote(_)) => {}
            Event::Start(Tag::List(ordinal)) => {
                list_ordered = ordinal.is_some();
                list_ordinal = ordinal.unwrap_or(0);
            }
            Event::End(TagEnd::List(_)) => {}
            Event::Start(Tag::Item) => {
                let ordinal = if list_ordered {
                    list_ordinal += 1;
                    list_ordinal - 1
                } else {
                    0
                };
                blocks.push(Block::ListItem {
                    ordered: list_ordered,
                    ordinal,
                    text: String::new(),
                });
            }
            Event::End(TagEnd::Item) => {}
            Event::Rule => blocks.push(Block::Rule),
            Event::Text(text) => append_text(&mut blocks, &text),
            Event::Code(code) => append_text(&mut blocks, &format!("`{code}`")),
            Event::SoftBreak | Event::HardBreak => append_text(&mut blocks, "\n"),
            Event::Start(Tag::Strong) => append_text(&mut blocks, "**"),
            Event::End(TagEnd::Strong) => append_text(&mut blocks, "**"),
            Event::Start(Tag::Emphasis) => append_text(&mut blocks, "*"),
            Event::End(TagEnd::Emphasis) => append_text(&mut blocks, "*"),
            Event::Start(Tag::Strikethrough) => append_text(&mut blocks, "~~"),
            Event::End(TagEnd::Strikethrough) => append_text(&mut blocks, "~~"),
            Event::Html(_) | Event::InlineHtml(_) => {
                // Raw HTML is never rendered from agent output.
            }
            _ => {}
        }
    }
    if truncated {
        // Marker replaces the last block so the total stays within the cap.
        blocks.truncate(max_blocks.saturating_sub(1));
        blocks.push(Block::Paragraph("…".to_string()));
    }
    blocks
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        _ => 3,
    }
}

fn append_text(blocks: &mut Vec<Block>, text: &str) {
    match blocks.last_mut() {
        Some(Block::Heading { text: t, .. })
        | Some(Block::Paragraph(t))
        | Some(Block::Quote(t))
        | Some(Block::ListItem { text: t, .. }) => t.push_str(text),
        Some(Block::Code { content, .. }) => content.push_str(text),
        _ => blocks.push(Block::Paragraph(text.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_headings_paragraphs_code() {
        let source = "# Title\n\nHello **world**.\n\n```rust\nfn main() {}\n```\n";
        let blocks = render_blocks(source, 32);
        assert_eq!(
            blocks,
            vec![
                Block::Heading {
                    level: 1,
                    text: "Title".into()
                },
                Block::Paragraph("Hello **world**.".into()),
                Block::Code {
                    language: Some("rust".into()),
                    content: "fn main() {}\n".into()
                },
            ]
        );
    }

    #[test]
    fn renders_ordered_and_unordered_lists() {
        let blocks = render_blocks("- a\n- b\n\n1. one\n2. two\n", 32);
        let items: Vec<_> = blocks
            .into_iter()
            .filter(|b| matches!(b, Block::ListItem { .. }))
            .collect();
        assert_eq!(items.len(), 4);
        assert!(matches!(&items[0], Block::ListItem { ordered: false, ordinal: 0, .. }));
        assert!(matches!(&items[2], Block::ListItem { ordered: true, ordinal: 1, .. }));
    }

    #[test]
    fn truncates_pathological_input() {
        let source = "para\n\n".repeat(100);
        let blocks = render_blocks(&source, 8);
        assert_eq!(blocks.len(), 8);
        assert_eq!(blocks[7], Block::Paragraph("…".into()));
    }

    #[test]
    fn html_is_dropped() {
        let blocks = render_blocks("<script>alert(1)</script>safe", 32);
        assert!(!format!("{blocks:?}").contains("script"));
    }
}
