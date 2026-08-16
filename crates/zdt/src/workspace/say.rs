//! The one line the status bar shows.

use super::*;

impl Workspace {
    // ---- Saying things ----------------------------------------------------------------------

    /// Says something in the status line.
    pub fn say(&self, text: impl Into<String>) {
        self.inner.message.set(Some(Message {
            text: text.into(),
            error: false,
        }));
    }

    /// Complains in the status line.
    pub fn complain(&self, text: impl Into<String>) {
        let text = text.into();
        tracing::warn!("{text}");
        self.inner.message.set(Some(Message { text, error: true }));
    }

    /// Takes back whatever was being said.
    pub fn hush(&self) {
        if self.inner.message.get_untracked().is_some() {
            self.inner.message.set(None);
        }
    }
}
