pub const TERMS: &str = include_str!("../legal/terms.md");
pub const PRIVACY: &str = include_str!("../legal/privacy.md");
pub const LICENSE: &str = include_str!("../legal/license.md");

pub enum MarkdownBlock {
    Heading { level: usize, text: String },
    Paragraph(String),
    Bullet(String),
}

/// Parse the small Markdown subset used by the bundled offline legal documents.
pub fn parse_markdown(source: &str) -> Vec<MarkdownBlock> {
    let mut blocks = Vec::new();
    let mut paragraph = String::new();

    fn flush(blocks: &mut Vec<MarkdownBlock>, paragraph: &mut String) {
        let text = paragraph.trim();
        if !text.is_empty() {
            blocks.push(MarkdownBlock::Paragraph(text.to_owned()));
        }
        paragraph.clear();
    }

    for line in source.lines() {
        let line = line.trim();
        if line.is_empty() {
            flush(&mut blocks, &mut paragraph);
        } else if let Some(text) = line.strip_prefix("### ") {
            flush(&mut blocks, &mut paragraph);
            blocks.push(MarkdownBlock::Heading {
                level: 3,
                text: text.to_owned(),
            });
        } else if let Some(text) = line.strip_prefix("## ") {
            flush(&mut blocks, &mut paragraph);
            blocks.push(MarkdownBlock::Heading {
                level: 2,
                text: text.to_owned(),
            });
        } else if let Some(text) = line.strip_prefix("# ") {
            flush(&mut blocks, &mut paragraph);
            blocks.push(MarkdownBlock::Heading {
                level: 1,
                text: text.to_owned(),
            });
        } else if let Some(text) = line.strip_prefix("- ") {
            flush(&mut blocks, &mut paragraph);
            blocks.push(MarkdownBlock::Bullet(text.to_owned()));
        } else {
            if !paragraph.is_empty() {
                paragraph.push(' ');
            }
            paragraph.push_str(line);
        }
    }
    flush(&mut blocks, &mut paragraph);
    blocks
}

#[cfg(test)]
mod tests {
    use super::{MarkdownBlock, TERMS, parse_markdown};

    #[test]
    fn bundled_terms_include_the_full_document_sections() {
        let blocks = parse_markdown(TERMS);
        let headings = blocks
            .iter()
            .filter_map(|block| match block {
                MarkdownBlock::Heading { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();

        for expected in [
            "nexawal Terms of Use",
            "Self-custody",
            "No warranties",
            "Limitation of liability",
            "Nodes and network",
            "Optional features",
            "License and source",
            "Unaudited software",
            "Changes",
            "Contact",
        ] {
            assert!(
                headings.contains(&expected),
                "missing Terms section: {expected}"
            );
        }
        assert!(blocks.iter().any(|block| matches!(
            block,
            MarkdownBlock::Paragraph(text) if text.contains("https://nexatrode.com/terms")
        )));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Document {
    Terms,
    Privacy,
    License,
}

impl Document {
    pub fn title(self) -> &'static str {
        match self {
            Self::Terms => "Terms of Use",
            Self::Privacy => "Privacy Policy",
            Self::License => "License",
        }
    }

    pub fn body(self) -> &'static str {
        match self {
            Self::Terms => TERMS,
            Self::Privacy => PRIVACY,
            Self::License => LICENSE,
        }
    }
}
