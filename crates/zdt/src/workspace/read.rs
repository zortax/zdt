//! What the interface reads.

use super::*;

impl Workspace {
    // ---- Reading ---------------------------------------------------------------------------

    /// The buffer line's order. Tracked.
    pub fn order(&self) -> Vec<BufferId> {
        self.inner.order.get()
    }

    /// The arrangement of windows. Tracked.
    pub fn layout(&self) -> Layout {
        self.inner.layout.get()
    }

    /// The arrangement of windows, without the shares. Tracked.
    pub fn shape(&self) -> Shape {
        self.inner.layout.with(Layout::shape)
    }

    /// The whole layout, without subscribing.
    pub fn layout_untracked(&self) -> Layout {
        self.inner.layout.get_untracked()
    }

    /// Which window has the keyboard. Tracked.
    pub fn focused(&self) -> WindowId {
        self.inner.focused.get()
    }

    /// Which window has the keyboard, without subscribing.
    pub fn focused_untracked(&self) -> WindowId {
        self.inner.focused.get_untracked()
    }

    /// What the interface is saying. Tracked.
    pub fn message(&self) -> Option<Message> {
        self.inner.message.get()
    }

    /// Reads one buffer, when it is still open. Tracked.
    pub fn buffer(&self, id: BufferId) -> Option<Buffer> {
        self.inner.buffers.with(|buffers| buffers.get(id).cloned())
    }

    /// Reads one buffer without subscribing.
    pub fn buffer_untracked(&self, id: BufferId) -> Option<Buffer> {
        self.inner
            .buffers
            .with_untracked(|buffers| buffers.get(id).cloned())
    }

    /// Reads one window. Tracked.
    pub fn window(&self, id: WindowId) -> Option<WindowState> {
        self.inner.windows.with(|windows| windows.get(id).cloned())
    }

    /// Every window there is, without subscribing.
    ///
    /// For the few things that have to ask all of them, such as finding which window an editor
    /// handle belongs to. Drawing walks the layout instead.
    #[must_use]
    pub fn windows(&self) -> Vec<WindowId> {
        self.inner
            .windows
            .with_untracked(|windows| windows.keys().collect())
    }

    /// The buffer the focused window is showing. Tracked.
    pub fn current_buffer(&self) -> Option<Buffer> {
        let window = self.window(self.focused())?;
        self.buffer(window.current?)
    }

    /// The buffer `id` shows, without subscribing.
    pub fn buffer_in_untracked(&self, window: WindowId) -> Option<BufferId> {
        self.inner
            .windows
            .with_untracked(|windows| windows.get(window).and_then(|state| state.current))
    }

    /// The buffer at `path`, when it is already open.
    pub fn find_path(&self, path: &Path) -> Option<BufferId> {
        self.inner.buffers.with_untracked(|buffers| {
            buffers
                .iter()
                .find(|(_, buffer)| buffer.is_at(path))
                .map(|(id, _)| id)
        })
    }
}
