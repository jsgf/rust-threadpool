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

#[derive(Debug, Copy, Clone)]
struct Counters {
    count: usize,
    count_max: usize,
}

struct Sentinel<'a> {
    jobs: &'a Arc<Mutex<Receiver<Thunk<'static>>>>,
    counters: &'a Arc<Mutex<Counters>>,
    active: bool
}

impl<'a> Sentinel<'a> {
    fn new(jobs: &'a Arc<Mutex<Receiver<Thunk<'static>>>>,
           counters: &'a Arc<Mutex<Counters>>) -> Sentinel<'a> {
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
            self.counters.lock().unwrap().count -= 1;
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
    counters: Arc<Mutex<Counters>>,
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
        let counts = Arc::new(Mutex::new(Counters { count: 0, count_max: threads }));

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
        self.counters.lock().unwrap().count
    }

    /// Returns the number of created threads
    pub fn max_count(&self) -> usize {
        self.counters.lock().unwrap().count_max
    }

    /// Sets the number of threads to use as `threads`.
    /// Can be used to change the threadpool size during runtime
    pub fn set_threads(&mut self, threads: usize) {
        assert!(threads >= 1);

        let mut current_max = threads;
        {
            use std::mem;
            let mut counters = self.counters.lock().unwrap();
            mem::swap(&mut counters.count_max, &mut current_max)
        }

        if threads > current_max {
            // Spawn new threads
            for _ in 0..(threads - current_max) {
                spawn_in_pool(self.job_receiver.clone(), self.counters.clone());
            }
        }
    }
}

fn spawn_in_pool(jobs: Arc<Mutex<Receiver<Thunk<'static>>>>,
                 counts: Arc<Mutex<Counters>>) {
    thread::spawn(move || {
        // Will spawn a new thread on panic unless it is cancelled.
        let sentinel = Sentinel::new(&jobs, &counts);

        loop {
            // clone values so that the mutexes are not held
            let Counters { count, count_max } = *counts.lock().unwrap();
            if count < count_max {
                let message = {
                    // Only lock jobs for the time it takes
                    // to get a job, not run it.
                    let lock = jobs.lock().unwrap();
                    lock.recv()
                };

                match message {
                    Ok(job) => {
                        counts.lock().unwrap().count += 1;
                        job.call_box();
                        counts.lock().unwrap().count -= 1;
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
