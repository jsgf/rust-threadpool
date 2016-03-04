// Copyright 2014 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Abstraction of a thread pool for basic parallelism.

use std::sync::mpsc::{channel, Sender, Receiver};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

trait FnBox {
    fn call_box(self: Box<Self>);
}

impl<F: FnOnce()> FnBox for F {
    fn call_box(self: Box<F>) {
        (*self)()
    }
}

type Thunk<'a> = Box<FnBox + Send + 'a>;

#[derive(Debug)]
struct Counters {
    count: AtomicUsize,
    count_max: AtomicUsize,
}

impl Clone for Counters {
    fn clone(&self) -> Self {
        // non-atomic
        Counters {
            count: AtomicUsize::new(self.count.load(Ordering::Relaxed)),
            count_max: AtomicUsize::new(self.count_max.load(Ordering::Relaxed)),
        }
    }
}

struct Sentinel<'a> {
    jobs: &'a Arc<Mutex<Receiver<Thunk<'static>>>>,
    counters: &'a Arc<Counters>,
    active: bool
}

impl<'a> Sentinel<'a> {
    fn new(jobs: &'a Arc<Mutex<Receiver<Thunk<'static>>>>,
           counters: &'a Arc<Counters>) -> Sentinel<'a> {
        Sentinel {
            jobs: jobs,
            counters: counters,
            active: true
        }
    }

    // Cancel and destroy this sentinel.
    fn cancel(mut self) {
        self.active = false;
    }
}

impl<'a> Drop for Sentinel<'a> {
    fn drop(&mut self) {
        if self.active {
            self.counters.count.fetch_sub(1, Ordering::Relaxed);
            spawn_in_pool(self.jobs.clone(), self.counters.clone())
        }
    }
}

/// A thread pool used to execute functions in parallel.
///
/// Spawns `n` worker threads and replenishes the pool if any worker threads
/// panic.
///
/// # Example
///
/// ```
/// use threadpool::ThreadPool;
/// use std::sync::mpsc::channel;
///
/// let pool = ThreadPool::new(4);
///
/// let (tx, rx) = channel();
/// for i in 0..8 {
///     let tx = tx.clone();
///     pool.execute(move|| {
///         tx.send(i).unwrap();
///     });
/// }
///
/// assert_eq!(rx.iter().take(8).fold(0, |a, b| a + b), 28);
/// ```
#[derive(Clone)]
pub struct ThreadPool {
    // How the threadpool communicates with subthreads.
    //
    // This is the only such Sender, so when it is dropped all subthreads will
    // quit.
    jobs: Sender<Thunk<'static>>,
    job_receiver: Arc<Mutex<Receiver<Thunk<'static>>>>,
    counters: Arc<Counters>,
}

impl ThreadPool {
    /// Spawns a new thread pool with `threads` threads.
    ///
    /// # Panics
    ///
    /// This function will panic if `threads` is 0.
    pub fn new(threads: usize) -> ThreadPool {
        assert!(threads >= 1);

        let (tx, rx) = channel::<Thunk<'static>>();
        let rx = Arc::new(Mutex::new(rx));
        let counts = Arc::new(Counters { count: AtomicUsize::new(0), count_max: AtomicUsize::new(threads) });

        // Threadpool threads
        for _ in 0..threads {
            spawn_in_pool(rx.clone(), counts.clone());
        }

        ThreadPool {
            jobs: tx,
            job_receiver: rx.clone(),
            counters: counts,
        }
    }

    /// Executes the function `job` on a thread in the pool.
    pub fn execute<F>(&self, job: F)
        where F : FnOnce() + Send + 'static
    {
        self.jobs.send(Box::new(move || job())).unwrap();
    }

    /// Returns the number of currently active threads.
    pub fn active_count(&self) -> usize {
        self.counters.count.load(Ordering::Relaxed)
    }

    /// Returns the number of created threads
    pub fn max_count(&self) -> usize {
        self.counters.count_max.load(Ordering::Relaxed)
    }

    /// Sets the number of threads to use as `threads`.
    /// Can be used to change the threadpool size during runtime
    pub fn set_threads(&mut self, threads: usize) {
        assert!(threads >= 1);

        let current_max = self.counters.count_max.swap(threads, Ordering::Relaxed);

        if threads > current_max {
            // Spawn new threads
            for _ in 0..(threads - current_max) {
                spawn_in_pool(self.job_receiver.clone(), self.counters.clone());
            }
        }
    }
}

fn spawn_in_pool(jobs: Arc<Mutex<Receiver<Thunk<'static>>>>,
                 counts: Arc<Counters>) {
    thread::spawn(move || {
        // Will spawn a new thread on panic unless it is cancelled.
        let sentinel = Sentinel::new(&jobs, &counts);

        loop {
            // clone values so that the mutexes are not held
            let count = counts.count.load(Ordering::Relaxed);
            let count_max = counts.count_max.load(Ordering::Relaxed);

            if count < count_max {
                let message = {
                    // Only lock jobs for the time it takes
                    // to get a job, not run it.
                    let lock = jobs.lock().unwrap();
                    lock.recv()
                };

                match message {
                    Ok(job) => {
                        counts.count.fetch_add(1, Ordering::Relaxed);
                        job.call_box();
                        counts.count.fetch_sub(1, Ordering::Relaxed);
                    },

                    // The Threadpool was dropped.
                    Err(..) => break
                }
            } else {
                break;
            }
        }

        sentinel.cancel();
    });
}
