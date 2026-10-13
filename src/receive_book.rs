//! Persisted receive subaddress book (account 0), matching iOS/Android.

use crate::paths;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub index: u32,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Book {
    pub next_index: u32,
    pub selected: u32,
    pub entries: Vec<Entry>,
}

impl Book {
    /// Publish edits only after the atomic file replacement succeeds.
    pub fn update_and_save<T>(&mut self, update: impl FnOnce(&mut Self) -> T) -> std::io::Result<T> {
        // Do not overwrite an unreadable, corrupt, or externally changed book with the
        // in-memory copy. In particular, absence is not interchangeable with corruption.
        if load()? != *self {
            return Err(std::io::Error::other("Receive address book changed; reopen the wallet before editing it."));
        }
        self.update_with_writer(update, save)
    }

    fn update_with_writer<T>(&mut self, update: impl FnOnce(&mut Self) -> T,
        write: impl FnOnce(&Self) -> std::io::Result<()>) -> std::io::Result<T> {
        let mut candidate = self.clone();
        let result = update(&mut candidate);
        write(&candidate)?;
        *self = candidate;
        Ok(result)
    }

    pub fn required_coverage(&self) -> u32 {
        self.entries.iter().map(|entry| entry.index).max().unwrap_or(0)
            .max(self.next_index.saturating_sub(1)).max(50)
    }

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
        if entry.index == 0 && (label.is_empty() || label == "Primary") {
            crate::l10n::t("Primary").to_string()
        } else if label.is_empty() {
            crate::l10n::interpolate("Subaddress {0}", &[entry.index.to_string()])
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

pub fn load() -> std::io::Result<Book> {
    load_from(&paths::receive_book_path())
}

fn load_from(path: &std::path::Path) -> std::io::Result<Book> {
    match std::fs::read_to_string(path) {
        Ok(raw) => parse(&raw).ok_or_else(|| std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Receive address book is invalid; restore its backup before reopening the wallet.",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // A dangling symlink is an existing damaged book, not a fresh installation.
            match std::fs::symlink_metadata(path) {
                Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => Ok(Book::primary()),
                _ => Err(error),
            }
        }
        Err(error) => Err(error),
    }
}

fn save(book: &Book) -> std::io::Result<()> {
    paths::write_bytes(paths::receive_book_path(), format_book(book).as_bytes())
}

pub fn clear() {
    let _ = std::fs::remove_file(paths::receive_book_path());
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
    entries.sort_by_key(|e| e.index);
    if !entries.iter().any(|e| e.index == 0)
        || !entries.iter().any(|e| e.index == selected)
        || entries.windows(2).any(|pair| pair[0].index == pair[1].index) {
        return None;
    }
    let max_index = entries.iter().map(|e| e.index).max().unwrap_or(0);
    if next_index < max_index.checked_add(1)? || next_index == u32::MAX { return None; }
    Some(Book {
        next_index,
        selected,
        entries,
    })
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
    fn missing_book_is_new_but_corruption_and_read_errors_are_not() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("book");
        assert_eq!(load_from(&path).unwrap(), Book::primary());
        for bytes in [b"".as_slice(), b"corrupt", b"2\n1\n0|Primary\nnot-an-index|Shop\n",
            b"4294967295\n0\n4294967295|Overflow\n", b"2\n0\n", b"2\n1\n0|Primary\n",
            b"2\n0\n0|Primary\n0|Duplicate\n", b"1\n1\n0|Primary\n1|Shop\n"] {
            std::fs::write(&path, bytes).unwrap();
            assert_eq!(load_from(&path).unwrap_err().kind(), std::io::ErrorKind::InvalidData);
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        // A directory in place of the file is a read error, not a new book.
        assert!(load_from(directory.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlink_is_not_an_absent_book() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("book");
        std::os::unix::fs::symlink(directory.path().join("missing-target"), &path).unwrap();
        assert!(load_from(&path).is_err());
        assert!(std::fs::symlink_metadata(&path).unwrap().file_type().is_symlink());
    }

    #[test]
    fn failed_save_does_not_issue_address_or_advance_book() {
        let mut book = Book::primary();
        book.next_index = 51;
        let original = book.clone();
        let result = book.update_with_writer(|b| b.allocate_new("Shop"), |_| {
            Err(std::io::Error::other("disk full"))
        });
        assert!(result.is_err());
        assert_eq!(book, original);
        let mut persisted = String::new();
        let index = book.update_with_writer(|b| b.allocate_new("Shop"), |b| {
            persisted = format_book(b); Ok(())
        }).unwrap();
        assert_eq!(index, 51);
        assert_eq!(parse(&persisted).unwrap(), book);
        assert_eq!(book.required_coverage(), 51);
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
