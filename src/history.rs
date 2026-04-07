//! Prompt history ring buffer.
//!
//! Convention (matches every Unix shell):
//!   Up   (prev) -- move toward older entries (lower index)
//!   Down (next) -- move toward newer entries (higher index / live input)

#[derive(Debug, Default)]
pub struct History {
    entries: Vec<String>,
    /// `None` = user is at the live input line (not navigating).
    cursor: Option<usize>,
    max: usize,
}

impl History {
    pub fn new(max: usize) -> Self {
        Self {
            entries: Vec::with_capacity(max.min(1024)),
            cursor: None,
            max,
        }
    }

    /// Pushes a prompt. Ignores empty strings and consecutive duplicates.
    pub fn push(&mut self, prompt: impl Into<String>) {
        let s = prompt.into();
        if s.trim().is_empty() {
            return;
        }
        if self.entries.last().map(|e| e == &s).unwrap_or(false) {
            return;
        }
        if self.entries.len() == self.max {
            self.entries.remove(0);
        }
        self.entries.push(s);
        self.cursor = None;
    }

    /// Navigate toward older entries (Up arrow).
    /// First call returns the newest; stays pinned at oldest.
    pub fn prev(&mut self) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }
        let next_cursor = match self.cursor {
            None => self.entries.len() - 1,
            Some(0) => 0,
            Some(n) => n - 1,
        };
        self.cursor = Some(next_cursor);
        Some(&self.entries[next_cursor])
    }

    /// Navigate toward newer entries (Down arrow).
    /// Returns `None` when past the newest entry (back to live input).
    pub fn next(&mut self) -> Option<&str> {
        match self.cursor {
            None => None,
            Some(n) if n + 1 >= self.entries.len() => {
                self.cursor = None;
                None
            }
            Some(n) => {
                self.cursor = Some(n + 1);
                Some(&self.entries[n + 1])
            }
        }
    }

    pub fn reset_cursor(&mut self) {
        self.cursor = None;
    }

    pub fn entries(&self) -> &[String] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_prev_newest_first() {
        let mut h = History::new(10);
        h.push("first");
        h.push("second");
        assert_eq!(h.prev(), Some("second"));
        assert_eq!(h.prev(), Some("first"));
        assert_eq!(h.prev(), Some("first")); // stays pinned
    }

    #[test]
    fn down_returns_newer_entries() {
        let mut h = History::new(10);
        h.push("a");
        h.push("b");
        h.push("c");
        h.prev(); // c
        h.prev(); // b
        h.prev(); // a
        assert_eq!(h.next(), Some("b"));
        assert_eq!(h.next(), Some("c"));
        assert_eq!(h.next(), None);
    }

    #[test]
    fn down_from_live_is_noop() {
        let mut h = History::new(10);
        h.push("hello");
        assert_eq!(h.next(), None);
    }

    #[test]
    fn prev_then_next_returns_to_live() {
        let mut h = History::new(10);
        h.push("a");
        h.push("b");
        h.prev();
        assert_eq!(h.next(), None);
    }

    #[test]
    fn respects_max_capacity() {
        let mut h = History::new(3);
        h.push("1");
        h.push("2");
        h.push("3");
        h.push("4");
        assert_eq!(h.entries().len(), 3);
        assert_eq!(h.entries()[0], "2");
    }

    #[test]
    fn ignores_consecutive_duplicates() {
        let mut h = History::new(10);
        h.push("hello");
        h.push("hello");
        assert_eq!(h.entries().len(), 1);
    }

    #[test]
    fn allows_non_consecutive_duplicate() {
        let mut h = History::new(10);
        h.push("hello");
        h.push("world");
        h.push("hello");
        assert_eq!(h.entries().len(), 3);
    }
}
