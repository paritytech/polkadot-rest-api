// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Concurrency utilities for running futures with controlled parallelism.

use futures::stream::{FuturesUnordered, StreamExt};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// Default number of concurrent tasks for RC block processing.
pub const DEFAULT_CONCURRENCY: usize = 4;

/// Runs futures with bounded concurrency, starting new tasks as running ones complete.
pub async fn run_with_concurrency<F, T, E>(
    max_concurrent: usize,
    tasks: impl IntoIterator<Item = F>,
) -> Result<Vec<T>, E>
where
    F: Future<Output = Result<T, E>>,
{
    let semaphore = Arc::new(Semaphore::new(max_concurrent));
    let mut futs = FuturesUnordered::new();

    for (idx, task) in tasks.into_iter().enumerate() {
        let sem = semaphore.clone();
        futs.push(async move {
            let _permit = sem.acquire().await.expect("semaphore closed unexpectedly");
            task.await.map(|val| (idx, val))
        });
    }

    let mut indexed_results = Vec::new();
    while let Some(result) = futs.next().await {
        indexed_results.push(result?);
    }
    indexed_results.sort_by_key(|(idx, _)| *idx);
    Ok(indexed_results.into_iter().map(|(_, val)| val).collect())
}
