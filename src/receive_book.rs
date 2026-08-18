//! Persisted receive subaddress book (account 0), matching iOS/Android.

use crate::paths;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub index: u32,
    pub label: String,
}

#[derive(Clone, Debug)]
pub struct Book {
    pub next_index: u32,
    pub selected: u32,
    pub entries: Vec<Entry>,
}

impl Book {
    pub fn primary() -> Self {
        Self {
            next_index: 1,
            selected: 0,
            entries: vec![Entry {
                index: 0,
                label: "Primary".into(),
            }],
        }
    }

    pub fn selected_entry(&self) -> &Entry {
        self.entries
            .iter()
            .find(|e| e.index == self.selected)
            .unwrap_or(&self.entries[0])
    }

    pub fn display_label(&self) -> String {
        self.display_label_for(self.selected)
    }

    pub fn display_label_for(&self, index: u32) -> String {
        let entry = self
            .entries
            .iter()
            .find(|e| e.index == index)
            .unwrap_or(&self.entries[0]);
        let label = entry.label.trim();
        if label.is_empty() {
            format!("Subaddress {}", entry.index)
        } else {
            label.to_string()
        }
    }

    pub fn cycle_index(&self, current: u32, next: bool) -> u32 {
        if self.entries.len() < 2 {
            return current;
        }
        let pos = self
            .entries
            .iter()
            .position(|e| e.index == current)
            .unwrap_or(0);
        let idx = if next {
            (pos + 1) % self.entries.len()
        } else {
            (pos + self.entries.len() - 1) % self.entries.len()
        };
        self.entries[idx].index
    }

    pub fn select_next(&mut self) {
        if self.entries.len() < 2 {
            return;
        }
        let pos = self
            .entries
            .iter()
            .position(|e| e.index == self.selected)
            .unwrap_or(0);
        let next = (pos + 1) % self.entries.len();
        self.selected = self.entries[next].index;
    }

    pub fn select_prev(&mut self) {
        if self.entries.len() < 2 {
            return;
        }
        let pos = self
            .entries
            .iter()
            .position(|e| e.index == self.selected)
            .unwrap_or(0);
        let prev = (pos + self.entries.len() - 1) % self.entries.len();
        self.selected = self.entries[prev].index;
    }

    pub fn allocate_new(&mut self, label: &str) -> u32 {
        let index = self.next_index.max(1);
        self.entries.push(Entry {
            index,
            label: label.trim().to_string(),
        });
        self.next_index = index + 1;
        self.selected = index;
        index
    }

    pub fn set_selected_label(&mut self, label: &str) {
        let selected = self.selected;
        if let Some(entry) = self.entries.iter_mut().find(|e| e.index == selected) {
            entry.label = label.trim().to_string();
        }
    }
}

pub fn load() -> Book {
    parse(&fs_read()).unwrap_or_else(Book::primary)
}

pub fn save(book: &Book) {
    let _ = paths::write_bytes(paths::receive_book_path(), format_book(book).as_bytes());
}

pub fn clear() {
    let _ = std::fs::remove_file(paths::receive_book_path());
}

fn fs_read() -> String {
    std::fs::read_to_string(paths::receive_book_path()).unwrap_or_default()
}

fn format_book(book: &Book) -> String {
    let mut out = format!("{}\n{}\n", book.next_index, book.selected);
    for entry in &book.entries {
        out.push_str(&format!(
            "{}|{}\n",
            entry.index,
            entry.label.replace('|', " ").replace('\n', " ")
        ));
    }
    out
}

fn parse(raw: &str) -> Option<Book> {
    let mut lines = raw.lines().filter(|l| !l.trim().is_empty());
    let next_index: u32 = lines.next()?.trim().parse().ok()?;
    let selected: u32 = lines.next()?.trim().parse().ok()?;
    let mut entries = Vec::new();
    for line in lines {
        let (index_raw, label) = line.split_once('|').unwrap_or((line, ""));
        let index: u32 = index_raw.trim().parse().ok()?;
        entries.push(Entry {
            index,
            label: label.trim().to_string(),
        });
    }
    if !entries.iter().any(|e| e.index == 0) {
        entries.insert(
            0,
            Entry {
                index: 0,
                label: "Primary".into(),
            },
        );
    }
    entries.sort_by_key(|e| e.index);
    entries.dedup_by_key(|e| e.index);
    let max_index = entries.iter().map(|e| e.index).max().unwrap_or(0);
    let mut book = Book {
        next_index: next_index.max(max_index + 1),
        selected,
        entries,
    };
    if !book.entries.iter().any(|e| e.index == book.selected) {
        book.selected = 0;
    }
    Some(book)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut book = Book::primary();
        book.allocate_new("Shop");
        let parsed = parse(&format_book(&book)).unwrap();
        assert_eq!(parsed.next_index, 2);
        assert_eq!(parsed.selected, 1);
        assert_eq!(parsed.entries[1].label, "Shop");
    }

    #[test]
    fn cycle_index_wraps() {
        let mut book = Book::primary();
        book.allocate_new("Shop");
        book.allocate_new("Tips");
        assert_eq!(book.cycle_index(0, true), 1);
        assert_eq!(book.cycle_index(2, true), 0);
        assert_eq!(book.cycle_index(0, false), 2);
        assert_eq!(book.display_label_for(1), "Shop");
    }
}
