//! Cancelling, pausing and stepping a run from another thread.

use std::collections::HashSet;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// Why a run stopped to wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PauseReason {
    /// [`RunControl::pause`] was called.
    Requested,
    /// Step mode: the run waits before every block.
    Step,
    /// The block has a breakpoint.
    Breakpoint,
    /// A person moved the mouse or typed while the test was driving the computer.
    UserInput,
}

/// Remote control of a run; clones control the same run. Every method takes effect at once:
/// a run waiting for the screen wakes up on [`cancel`](Self::cancel) without finishing the wait.
#[derive(Debug, Clone, Default)]
pub struct RunControl(Arc<Shared>);

#[derive(Debug, Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

#[derive(Debug, Default)]
struct State {
    cancelled: bool,
    pause_requested: bool,
    paused: bool,
    step_mode: bool,
    breakpoints: HashSet<String>,
}

/// The run was cancelled.
#[derive(Debug)]
pub(crate) struct Cancelled;

impl RunControl {
    pub fn new() -> Self {
        Self::default()
    }

    /// Emergency stop: the run ends before the next action and is reported as cancelled.
    pub fn cancel(&self) {
        self.update(|s| s.cancelled = true);
    }

    pub fn is_cancelled(&self) -> bool {
        self.lock().cancelled
    }

    /// Pauses before the next block, or between two looks at the screen while waiting.
    pub fn pause(&self) {
        self.update(|s| s.pause_requested = true);
    }

    /// Leaves step mode and continues until the next breakpoint.
    pub fn resume(&self) {
        self.update(|s| {
            s.paused = false;
            s.pause_requested = false;
            s.step_mode = false;
        });
    }

    /// Runs one block and pauses before the next; turns step mode on.
    pub fn step(&self) {
        self.update(|s| {
            s.paused = false;
            s.pause_requested = false;
            s.step_mode = true;
        });
    }

    /// With step mode on before the run starts, it pauses before the first block.
    pub fn set_step_mode(&self, on: bool) {
        self.update(|s| s.step_mode = on);
    }

    /// Ids of blocks to pause before, in any test of the run.
    pub fn set_breakpoints<I: IntoIterator<Item = String>>(&self, blocks: I) {
        let blocks = blocks.into_iter().collect();
        self.update(|s| s.breakpoints = blocks);
    }

    pub fn is_paused(&self) -> bool {
        self.lock().paused
    }

    /// Decides whether to pause here and, if so, marks the run paused. Step mode and
    /// breakpoints apply only before a block, not between looks at the screen.
    pub(crate) fn pause_reason(
        &self,
        block: &str,
        before_block: bool,
    ) -> Result<Option<PauseReason>, Cancelled> {
        let mut s = self.lock();
        if s.cancelled {
            return Err(Cancelled);
        }
        let reason = if before_block && s.step_mode {
            Some(PauseReason::Step)
        } else if before_block && s.breakpoints.contains(block) {
            Some(PauseReason::Breakpoint)
        } else if s.pause_requested {
            Some(PauseReason::Requested)
        } else {
            None
        };
        if reason.is_some() {
            s.paused = true;
            s.pause_requested = false;
        }
        Ok(reason)
    }

    pub(crate) fn enter_pause(&self) {
        self.update(|s| s.paused = true);
    }

    /// Blocks until resumed, stepped or cancelled.
    pub(crate) fn wait_while_paused(&self) -> Result<(), Cancelled> {
        let s = self
            .0
            .wake
            .wait_while(self.lock(), |s| s.paused && !s.cancelled)
            .unwrap_or_else(PoisonError::into_inner);
        if s.cancelled { Err(Cancelled) } else { Ok(()) }
    }

    /// Sleeps for `time` unless cancelled first.
    pub(crate) fn sleep(&self, time: Duration) -> Result<(), Cancelled> {
        let (s, _) = self
            .0
            .wake
            .wait_timeout_while(self.lock(), time, |s| !s.cancelled)
            .unwrap_or_else(PoisonError::into_inner);
        if s.cancelled { Err(Cancelled) } else { Ok(()) }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn update(&self, change: impl FnOnce(&mut State)) {
        change(&mut self.lock());
        self.0.wake.notify_all();
    }
}
