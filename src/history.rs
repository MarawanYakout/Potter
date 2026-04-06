//! Prompt history ring buffer.
//!
//! Stores the last N prompts in memory. Supports navigating with ↑/↓
//! arrow keys just like a terminal shell history.

/// A fixed-capacity ring buffer of prompt strings.
#[derive(Debug, Default)]
pub struct History {
    entries: Vec<String>,
    /// Cursor position when navigating (None = at the live input)
    cursor: Option<usize>,
    max: usize,
}

impl History {
    /// Creates a new `History` with the given capacity.
    pub fn new(max: usize) -> Self {
        Self {
            entries: Vec::with_capacity(max.min(1024)),
            cursor: None,
            max,
        }
    }

    /// Pushes a new prompt. Ignores empty strings and duplicates of the last entry.
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
        self.cursor = None; // reset navigation after new push
    }

    /// Navigate backwards (↑). Returns the previous prompt or `None`.
    pub fn prev(&mut self) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }
        let next = match self.cursor {
            None => self.entries.len() - 1,
            Some(0) => 0,
            Some(n) => n - 1,
        };
        self.cursor = Some(next);
        Some(&self.entries[next])
    }

    /// Navigate forwards (↓). Returns the next prompt, or `None` when back at live input.
    pub fn next(&mut self) -> Option<&str> {
        match self.cursor {
            None | Some(0) => {
                self.cursor = None;
                None
            }
            Some(n) => {
                self.cursor = Some(n - 1);
                Some(&self.entries[n - 1])
            }
        }
    }

    /// Resets the navigation cursor (e.g., when the user starts typing).
    pub fn reset_cursor(&mut self) {
        self.cursor = None;
    }

    /// Returns all stored entries (oldest first).
    pub fn entries(&self) -> &[String] {
        &self.entries
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_prev() {
        let mut h = History::new(10);
        h.push("first");
        h.push("second");
        assert_eq!(h.prev(), Some("second"));
        assert_eq!(h.prev(), Some("first"));
        assert_eq!(h.prev(), Some("first")); // stays at oldest
    }

    #[test]
    fn prev_then_next() {
        let mut h = History::new(10);
        h.push("a");
        h.push("b");
        h.prev(); // b
        h.prev(); // a
        assert_eq!(h.next(), Some("b"));
        assert_eq!(h.next(), None); // back to live input
    }

    #[test]
    fn respects_max_capacity() {
        let mut h = History::new(3);
        h.push("1");
        h.push("2");
        h.push("3");
        h.push("4"); // should evict "1"
        assert_eq!(h.entries().len(), 3);
        assert_eq!(h.entries()[0], "2");
    }

    #[test]
    fn ignores_duplicates_of_last() {
        let mut h = History::new(10);
        h.push("hello");
        h.push("hello");
        assert_eq!(h.entries().len(), 1);
    }
}
