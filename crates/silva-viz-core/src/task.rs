// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Deferred work whose result is collected from the UI loop.
//!
//! Reading a file is synchronous on both platforms — the web build's bytes are
//! already in memory by the time it has them. *acquiring* them is not: a
//! browser's file picker is a promise, and `eframe`'s `update()` cannot await.
//! So exactly one thing here is async, and this is the shape it takes.

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

/// Work that may finish on a later frame.
pub struct Task<T> {
    slot: Rc<RefCell<Option<T>>>,
}

impl<T: 'static> Default for Task<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: 'static> Task<T> {
    pub fn new() -> Self {
        Self {
            slot: Rc::new(RefCell::new(None)),
        }
    }

    /// Starts `future` and asks `ctx` for a repaint when it lands.
    ///
    /// The repaint is the whole reason `ctx` is here. [`Task::poll`] only runs
    /// inside `update()`, and eframe is reactive — it paints on input or on
    /// request and nothing else. A result that reaches the slot without waking
    /// the frame loop sits there, invisible, until the user happens to move the
    /// mouse. Asking here rather than at the call site means a new caller
    /// cannot forget.
    ///
    /// Starting a second task before the first is polled discards the first
    /// result, which is what should happen: it belongs to a superseded request.
    pub fn start<F>(&mut self, ctx: &egui::Context, future: F)
    where
        F: Future<Output = T> + 'static,
    {
        let slot = self.slot.clone();
        let ctx = ctx.clone();

        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(async move {
            *slot.borrow_mut() = Some(future.await);
            ctx.request_repaint();
        });

        #[cfg(not(target_arch = "wasm32"))]
        {
            // Native has a modal dialog rather than a promise, so blocking here
            // stalls only the frame the user spent in the file picker anyway.
            // The result still waits to be polled, so both platforms follow the
            // same path and a bug in the collection step cannot hide on one.
            *slot.borrow_mut() = Some(pollster::block_on(future));
            ctx.request_repaint();
        }
    }

    /// Takes the result if one is waiting, leaving the task empty.
    pub fn poll(&mut self) -> Option<T> {
        self.slot.borrow_mut().take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs empty passes until the context stops asking for another.
    ///
    /// A fresh `Context` wants its first paint, so "a repaint was requested"
    /// means nothing until it has settled. Bounded rather than `loop`: a
    /// context that never quiesces is a failure worth seeing, not a hang.
    fn quiesced() -> egui::Context {
        let ctx = egui::Context::default();
        for _ in 0..8 {
            if !ctx.has_requested_repaint() {
                return ctx;
            }
            let _ = ctx.run(Default::default(), |_| {});
        }
        panic!("context never settled, so this test cannot tell a new request from a stale one");
    }

    #[test]
    fn test_nothing_to_poll_before_starting() {
        let mut task: Task<u32> = Task::new();
        assert!(task.poll().is_none());
    }

    #[test]
    fn test_polling_consumes_the_result() {
        let ctx = egui::Context::default();
        let mut task = Task::new();
        task.start(&ctx, async { 7u32 });

        assert_eq!(task.poll(), Some(7));
        // Applying the same result twice would open the same file twice.
        assert_eq!(task.poll(), None);
    }

    #[test]
    fn test_restarting_discards_the_earlier_result() {
        let ctx = egui::Context::default();
        let mut task = Task::new();
        task.start(&ctx, async { 1u32 });
        task.start(&ctx, async { 2u32 });

        assert_eq!(task.poll(), Some(2));
    }

    #[test]
    fn test_starting_requests_a_repaint_so_the_result_is_collected() {
        let ctx = quiesced();
        let mut task = Task::new();
        task.start(&ctx, async { 7u32 });

        assert!(ctx.has_requested_repaint());
    }

    #[test]
    fn test_an_idle_task_asks_for_nothing() {
        // The other half of the bargain: the app stays reactive. Requesting a
        // repaint unconditionally would keep the frame loop awake forever.
        let ctx = quiesced();
        let mut task: Task<u32> = Task::new();

        assert!(task.poll().is_none());
        assert!(!ctx.has_requested_repaint());
    }
}
