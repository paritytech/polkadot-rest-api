// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Concurrency utilities for running futures with controlled parallelism.

use futures::stream::{FuturesUnordered, StreamExt};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// Default number of concurrent tasks for RC block processing.
pub const DEFAULT_CONCURRENCY: usize = 4;

/// Runs at most `max_concurrent` tasks at once, running the next tasks only
/// when currently running ones finish and free up space.
///
/// Returns a [`Stream`] of results so callers can process items as they arrive
/// and, if desired, stop early without waiting for remaining futures.
///
/// **Note:** All futures are wrapped and pushed into the internal
/// `FuturesUnordered` eagerly, so memory usage scales with the iterator
/// length, not `max_concurrent`. This is fine for bounded iterators (hundreds
/// of items) but not suitable for unbounded ones.
pub fn run_with_concurrency<F, O>(
    max_concurrent: usize,
    tasks: impl IntoIterator<Item = F>,
) -> impl futures::Stream<Item = O>
where
    F: Future<Output = O>,
{
    let semaphore = Arc::new(Semaphore::new(max_concurrent));
    let futs = FuturesUnordered::new();

    for task in tasks {
        let sem = semaphore.clone();
        futs.push(async move {
            let _permit = sem.acquire().await.expect("semaphore closed unexpectedly");
            task.await
        });
    }

    futs
}

/// Like [`run_with_concurrency`], but collects all results into an ordered `Vec`.
///
/// Preserves input order despite out-of-order completion. Short-circuits on the
/// first `Err`, propagating it to the caller.
///
/// Uses [`ExactSizeIterator`] to pre-allocate the output vector and avoid sorting.
/// Results are placed directly at their original index as they complete.
pub async fn run_with_concurrency_collect<F, T, E>(
    max_concurrent: usize,
    tasks: impl ExactSizeIterator<Item = F>,
) -> Result<Vec<T>, E>
where
    F: Future<Output = Result<T, E>>,
    T: Default,
{
    let mut out: Vec<T> = (0..tasks.len()).map(|_| T::default()).collect();

    let tasks = tasks
        .enumerate()
        .map(|(idx, task)| async move { task.await.map(|val| (idx, val)) });

    let mut futs = std::pin::pin!(run_with_concurrency(max_concurrent, tasks));
    while let Some(result) = futs.next().await {
        let (idx, val) = result?;
        out[idx] = val;
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    type BoxedFuture<T, E> = std::pin::Pin<Box<dyn Future<Output = Result<T, E>> + Send>>;

    #[tokio::test]
    async fn test_run_with_concurrency_collect_preserves_order() {
        // Tasks complete in reverse order but results should be in original order
        let tasks: Vec<BoxedFuture<i32, ()>> = vec![
            Box::pin(async {
                tokio::time::sleep(Duration::from_millis(30)).await;
                Ok(1)
            }),
            Box::pin(async {
                tokio::time::sleep(Duration::from_millis(20)).await;
                Ok(2)
            }),
            Box::pin(async {
                tokio::time::sleep(Duration::from_millis(10)).await;
                Ok(3)
            }),
        ];

        let results = run_with_concurrency_collect(3, tasks.into_iter())
            .await
            .unwrap();
        assert_eq!(results, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_run_with_concurrency_collect_empty() {
        let tasks: Vec<BoxedFuture<i32, ()>> = vec![];
        let results = run_with_concurrency_collect(4, tasks.into_iter())
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_run_with_concurrency_collect_single_item() {
        let tasks: Vec<BoxedFuture<i32, ()>> = vec![Box::pin(async { Ok(42) })];
        let results = run_with_concurrency_collect(4, tasks.into_iter())
            .await
            .unwrap();
        assert_eq!(results, vec![42]);
    }

    #[tokio::test]
    async fn test_run_with_concurrency_collect_error_propagation() {
        let tasks: Vec<BoxedFuture<i32, &str>> = vec![
            Box::pin(async { Ok(1) }),
            Box::pin(async { Err("error") }),
            Box::pin(async { Ok(3) }),
        ];

        let result = run_with_concurrency_collect(3, tasks.into_iter()).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "error");
    }

    #[tokio::test]
    async fn test_run_with_concurrency_collect_respects_concurrency_limit() {
        let active_count = Arc::new(AtomicUsize::new(0));
        let max_observed = Arc::new(AtomicUsize::new(0));

        let tasks: Vec<_> = (0..10)
            .map(|i| {
                let active = active_count.clone();
                let max_obs = max_observed.clone();
                Box::pin(async move {
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_obs.fetch_max(current, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    Ok::<_, ()>(i)
                }) as BoxedFuture<i32, ()>
            })
            .collect();

        let results = run_with_concurrency_collect(3, tasks.into_iter())
            .await
            .unwrap();

        // Results should be in order
        assert_eq!(results, (0..10).collect::<Vec<_>>());

        // Max concurrent should not exceed limit
        assert!(max_observed.load(Ordering::SeqCst) <= 3);
    }

    #[tokio::test]
    async fn test_run_with_concurrency_collect_many_items() {
        let tasks: Vec<BoxedFuture<i32, ()>> = (0..100)
            .map(|i| Box::pin(async move { Ok(i * 2) }) as _)
            .collect();

        let results = run_with_concurrency_collect(4, tasks.into_iter())
            .await
            .unwrap();
        let expected: Vec<_> = (0..100).map(|i| i * 2).collect();
        assert_eq!(results, expected);
    }
}
