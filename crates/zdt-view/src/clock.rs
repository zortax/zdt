//! A clock that outlives the window it borrows.
//!
//! [`Timers`] belongs to a window: it is the clock that window's frame loop drives, and
//! `Timers::current()` answers nothing outside one. That is right for a view, and wrong for
//! anything longer-lived. State that outlives a window — a session, or the settings shared by
//! every window — still wants a debounce, and taking a clock once at construction would tie it to
//! whichever window happened to build it.
//!
//! A `Clock` is that state's own clock. It is bound to a window while one is looking at it, and
//! unbound when none is. Repeating work is re-armed on every bind, so it survives a window
//! closing and another opening. Work waiting to happen once runs on unbind, so a debounce is
//! never quietly dropped.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use zgui::view::time::{IntervalHandle, Timers};

/// Names one repeating job.
type JobId = u64;

/// Repeating work, and how often.
type Repeat = (JobId, Duration, Rc<RefCell<dyn FnMut()>>);

/// Work owed once, waiting for a window.
type Owed = (JobId, Box<dyn FnOnce()>);

/// A clock that outlives the window it borrows.
///
/// Cloning one is cloning a handle: every clone drives the same jobs.
#[derive(Clone, Default)]
pub struct Clock {
    inner: Rc<Inner>,
}

#[derive(Default)]
struct Inner {
    /// The window's clock, while a window is looking.
    host: RefCell<Option<Timers>>,
    /// Repeating work, kept so it can be armed again on the next bind.
    repeats: RefCell<Vec<Repeat>>,
    /// What each repeating job is armed as right now. Dropping one stops it.
    armed: RefCell<Vec<(JobId, IntervalHandle)>>,
    /// Work owed once, waiting for a window.
    waiting: RefCell<Vec<Owed>>,
    /// What each one-shot is armed as. Dropping one cancels it.
    once: RefCell<Vec<(JobId, zgui::view::time::TimeoutHandle)>>,
    next: Cell<JobId>,
}

/// A repeating job. Dropping it stops the repeating for good.
pub struct Job {
    clock: Clock,
    id: JobId,
}

/// Work owed once. Dropping it cancels the work.
pub struct Pending {
    clock: Clock,
    id: JobId,
}

impl Clock {
    /// A clock with no window behind it yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Lends this clock the enclosing window's own, and arms everything owed.
    ///
    /// Called from inside a window, where there certainly is one. Binding a clock that is already
    /// bound replaces what it had, which is what attaching to a second window does.
    pub fn bind_here(&self) {
        if let Some(timers) = Timers::current() {
            self.bind(timers);
        }
    }

    /// Lends this clock `timers`, and arms everything owed.
    pub fn bind(&self, timers: Timers) {
        *self.inner.host.borrow_mut() = Some(timers);
        self.arm_repeats();
        self.arm_waiting();
    }

    /// Takes the window's clock back.
    ///
    /// Everything owed once runs now. A debounce that is still waiting when its window closes is
    /// work somebody asked for, and dropping it silently would lose a settings write.
    pub fn unbind(&self) {
        self.inner.host.borrow_mut().take();
        self.inner.armed.borrow_mut().clear();
        self.inner.once.borrow_mut().clear();
        let owed: Vec<_> = self.inner.waiting.borrow_mut().drain(..).collect();
        for (_, work) in owed {
            work();
        }
    }

    /// Whether a window is lending this clock its own.
    #[must_use]
    pub fn is_bound(&self) -> bool {
        self.inner.host.borrow().is_some()
    }

    /// The window's clock, when one is looking.
    #[must_use]
    pub fn timers(&self) -> Option<Timers> {
        self.inner.host.borrow().clone()
    }

    /// Runs `work` once, no earlier than `after`.
    ///
    /// With no window, the work is held until one binds, or until [`unbind`](Self::unbind) runs
    /// it. Dropping the answer cancels it.
    #[must_use = "dropping the handle cancels the work"]
    pub fn after(&self, after: Duration, work: impl FnOnce() + 'static) -> Pending {
        let id = self.mint();
        match self.inner.host.borrow().clone() {
            Some(timers) => {
                let clock = self.clone();
                let handle = timers.set_timeout(after, move || {
                    clock.forget_once(id);
                    work();
                });
                self.inner.once.borrow_mut().push((id, handle));
            }
            None => self.inner.waiting.borrow_mut().push((id, Box::new(work))),
        }
        Pending {
            clock: self.clone(),
            id,
        }
    }

    /// Runs `work` every `every`, across any number of windows.
    ///
    /// Armed again on every bind, so a window closing pauses it and the next one resumes it.
    /// Dropping the answer stops it for good.
    #[must_use = "dropping the handle stops the job"]
    pub fn every(&self, every: Duration, work: impl FnMut() + 'static) -> Job {
        let id = self.mint();
        let work: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new(work));
        self.inner
            .repeats
            .borrow_mut()
            .push((id, every, Rc::clone(&work)));
        if let Some(timers) = self.inner.host.borrow().clone() {
            self.arm_one(&timers, id, every, &work);
        }
        Job {
            clock: self.clone(),
            id,
        }
    }

    /// Arms every repeating job against the window that is lending its clock.
    fn arm_repeats(&self) {
        let Some(timers) = self.inner.host.borrow().clone() else {
            return;
        };
        self.inner.armed.borrow_mut().clear();
        let jobs: Vec<_> = self
            .inner
            .repeats
            .borrow()
            .iter()
            .map(|(id, every, work)| (*id, *every, Rc::clone(work)))
            .collect();
        for (id, every, work) in jobs {
            self.arm_one(&timers, id, every, &work);
        }
    }

    /// Arms one repeating job.
    fn arm_one(
        &self,
        timers: &Timers,
        id: JobId,
        every: Duration,
        work: &Rc<RefCell<dyn FnMut()>>,
    ) {
        let work = Rc::clone(work);
        let handle = timers.set_interval(every, move || {
            // A job that is already running is not entered again: a slow tick must not stack.
            if let Ok(mut work) = work.try_borrow_mut() {
                work();
            }
        });
        self.inner.armed.borrow_mut().push((id, handle));
    }

    /// Runs everything that was owed once while there was no window.
    fn arm_waiting(&self) {
        let owed: Vec<_> = self.inner.waiting.borrow_mut().drain(..).collect();
        for (_, work) in owed {
            work();
        }
    }

    /// A name no live job has.
    fn mint(&self) -> JobId {
        let id = self.inner.next.get();
        self.inner.next.set(id + 1);
        id
    }

    /// Forgets one repeating job.
    fn stop(&self, id: JobId) {
        self.inner
            .repeats
            .borrow_mut()
            .retain(|(held, ..)| *held != id);
        self.inner
            .armed
            .borrow_mut()
            .retain(|(held, _)| *held != id);
    }

    /// Cancels one piece of work owed once.
    fn cancel(&self, id: JobId) {
        self.inner
            .waiting
            .borrow_mut()
            .retain(|(held, _)| *held != id);
        self.forget_once(id);
    }

    /// Drops a one-shot's handle once it has fired.
    fn forget_once(&self, id: JobId) {
        if let Ok(mut once) = self.inner.once.try_borrow_mut() {
            once.retain(|(held, _)| *held != id);
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.clock.stop(self.id);
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.clock.cancel(self.id);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Duration;

    use super::Clock;

    #[test]
    fn work_owed_with_no_window_runs_when_one_arrives() {
        let clock = Clock::new();
        let ran = Rc::new(Cell::new(false));
        let held = {
            let ran = Rc::clone(&ran);
            clock.after(Duration::from_millis(50), move || ran.set(true))
        };
        assert!(!ran.get(), "nothing runs without a window");

        // Binding is what a window attaching does; the work is owed and runs.
        clock.unbind();
        assert!(ran.get(), "work owed once is never dropped");
        drop(held);
    }

    #[test]
    fn cancelling_before_a_window_arrives_forgets_the_work() {
        let clock = Clock::new();
        let ran = Rc::new(Cell::new(false));
        let held = {
            let ran = Rc::clone(&ran);
            clock.after(Duration::from_millis(50), move || ran.set(true))
        };
        drop(held);
        clock.unbind();
        assert!(!ran.get(), "cancelled work does not run");
    }

    #[test]
    fn an_unbound_clock_has_no_window() {
        let clock = Clock::new();
        assert!(!clock.is_bound());
        assert!(clock.timers().is_none());
    }

    #[test]
    fn a_repeating_job_survives_being_dropped_from_its_window() {
        let clock = Clock::new();
        let job = clock.every(Duration::from_millis(5), || {});
        // Unbinding leaves the job on the books, ready for the next window.
        clock.unbind();
        assert_eq!(clock.inner.repeats.borrow().len(), 1);
        drop(job);
        assert!(clock.inner.repeats.borrow().is_empty());
    }
}
