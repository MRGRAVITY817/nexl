//! Deterministic sequential executor for testing concurrent code.
//!
//! Implements the `sequential-executor` described in spec §10.5:
//!
//! > The sequential executor runs all forked tasks to completion in the
//! > order they were forked, without actual parallelism, enabling
//! > deterministic unit tests.
//!
//! `SequentialExecutor<T>` is a generic data structure: `T` is the task
//! return type.  In the eval layer, `T = Value`; in tests, any `Clone`
//! type works.

use std::collections::VecDeque;

/// A deterministic, sequential handler for the `Concurrent` effect.
///
/// Instead of spawning real threads, `fork` enqueues a thunk; `run_all`
/// drains the queue in FIFO order and returns all results.  Every task
/// completes before the next one starts.
pub struct SequentialExecutor<T> {
    tasks: VecDeque<Box<dyn FnOnce() -> T>>,
}

impl<T> SequentialExecutor<T> {
    /// Create an empty executor with no queued tasks.
    pub fn new() -> Self {
        Self {
            tasks: VecDeque::new(),
        }
    }

    /// Enqueue a thunk.  Returns the 0-based index of the queued task
    /// (usable as a deterministic task handle in tests).
    pub fn fork(&mut self, thunk: impl FnOnce() -> T + 'static) -> usize {
        let id = self.tasks.len();
        self.tasks.push_back(Box::new(thunk));
        id
    }

    /// Run all queued tasks to completion in FIFO order and return their
    /// results.  The queue is empty after this call.
    pub fn run_all(&mut self) -> Vec<T> {
        let mut results = Vec::with_capacity(self.tasks.len());
        while let Some(task) = self.tasks.pop_front() {
            results.push(task());
        }
        results
    }

    /// Number of tasks currently queued.
    pub fn pending(&self) -> usize {
        self.tasks.len()
    }
}

impl<T> Default for SequentialExecutor<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Test 1 --
    #[test]
    fn sequential_executor_fork_queues_tasks() {
        let mut exec: SequentialExecutor<i32> = SequentialExecutor::new();
        assert_eq!(exec.pending(), 0);

        let id0 = exec.fork(|| 10);
        assert_eq!(id0, 0);
        assert_eq!(exec.pending(), 1);

        let id1 = exec.fork(|| 20);
        assert_eq!(id1, 1);
        assert_eq!(exec.pending(), 2);
    }

    // -- Test 2 --
    #[test]
    fn sequential_executor_run_all_executes_fifo() {
        let mut exec: SequentialExecutor<i32> = SequentialExecutor::new();
        exec.fork(|| 1);
        exec.fork(|| 2);
        exec.fork(|| 3);

        let results = exec.run_all();
        // FIFO: tasks execute in the order they were forked
        assert_eq!(results, vec![1, 2, 3]);
        // Queue is drained after run_all
        assert_eq!(exec.pending(), 0);
    }

    // -- Test 3 --
    #[test]
    fn sequential_executor_run_all_empty_returns_empty() {
        let mut exec: SequentialExecutor<i32> = SequentialExecutor::new();
        let results = exec.run_all();
        assert!(results.is_empty());
    }
}
