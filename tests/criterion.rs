#![cfg(feature = "criterion")]
#![feature(plugin)]
#![plugin(criterion_macros)]

extern crate threadpool;
extern crate criterion;

use std::cmp::max;

use criterion::{Criterion, Bencher};

use threadpool::ThreadPool;
use std::sync::mpsc::{sync_channel, channel};
use std::sync::atomic::{AtomicUsize, Ordering, fence};
use std::sync::Arc;
use std::thread;

// Constant amount of work for worker to perform
#[inline]
fn work() {
    for _ in 0..10_000 { fence(Ordering::Acquire) }
}

#[criterion]
fn work_bench(b: &mut Bencher) {
    b.iter(|| work())
}

#[criterion]
fn func_bench(b: &mut Bencher) {
    // measure baseline mpsc perf
    let (tx, rx) = channel();

    b.iter_with_setup(|| tx.clone(),
        |tx| {
            work();
            let _ = tx.send(0u32).unwrap();
            assert_eq!(rx.recv().unwrap(), 0u32)
        })
}

#[criterion]
fn sync_func_bench(b: &mut Bencher) {
    // measure baseline mpsc perf
    let (tx, rx) = sync_channel(1);

    b.iter_with_setup(|| tx.clone(),
        |tx| {
            work();
            let _ = tx.send(0u32).unwrap();
            assert_eq!(rx.recv().unwrap(), 0u32)
        })
}

#[criterion]
fn atomic_func_bench(b: &mut Bencher) {
    // measure baseline atomic perf
    let count = Arc::new(AtomicUsize::new(0));

    b.iter_with_setup(|| count.clone(),
        |count| {
            let _ = count.fetch_add(1, Ordering::Relaxed);
            work();
            let _ = count.fetch_sub(1, Ordering::Relaxed);
            while count.load(Ordering::Relaxed) != 0 {}
        })
}

#[criterion]
fn atomic_thread_bench(b: &mut Bencher) {
    // measure baseline thread context switch/inter-core time
    let count = Arc::new(AtomicUsize::new(0));

    // Set up "worker"
    {
        let count = count.clone();
        thread::spawn(move || {
            // mark us running
            count.store(0xffff, Ordering::Release);
            // wait for ack
            while count.load(Ordering::Acquire) == 0xffff {}

            // worker
            loop {
                // wait for work
                while count.load(Ordering::Relaxed) == 0 {}

                // do it
                work();
                if count.fetch_sub(1, Ordering::Relaxed) == 0xfffe { break }
            }
        });
    }

    // wait for worker start
    while count.load(Ordering::Acquire) == 0 {}
    // ack seeing worker
    count.store(0, Ordering::Release);

    b.iter_with_setup(|| count.clone(),
        |count| {
            let count = count.clone();
            let _ = count.fetch_add(1, Ordering::Relaxed);
            while count.load(Ordering::Relaxed) != 0 {}
        });

    // Terminate worker
    count.store(0xfffe, Ordering::Release);
}

fn threadpool_bench(b: &mut Bencher, clients: usize, workers: usize) {
    let (tx, rx) = sync_channel(max(clients, workers));
    let pool = ThreadPool::new(workers);

    b.iter(|| {
        for _ in 0..clients {
            let tx = tx.clone();
            pool.execute(move || { work(); tx.send(0u32).unwrap() });
        }
        for _ in 0..clients {
            assert_eq!(rx.recv().unwrap(), 0u32)
        }
    })
}

#[criterion]
fn threadpool_bench_c1_w1(b: &mut Bencher) {
    threadpool_bench(b, 1, 1)
}

#[criterion]
fn threadpool_bench_c2_w1(b: &mut Bencher) {
    threadpool_bench(b, 2, 1)
}

#[criterion]
fn threadpool_bench_c2_w2(b: &mut Bencher) {
    threadpool_bench(b, 2, 2)
}

#[criterion]
fn threadpool_bench_c1_w2(b: &mut Bencher) {
    threadpool_bench(b, 1, 2)
}

#[criterion]
fn threadpool_bench_c20_w4(b: &mut Bencher) {
    threadpool_bench(b, 1, 2)
}

fn threadpool_bench_atomic(b: &mut Bencher, clients: usize, workers: usize) {
    let pool = ThreadPool::new(workers);

    b.iter(|| {
        let count = Arc::new(AtomicUsize::new(0));
        for _ in 0..clients {
            let count = count.clone();
            let _ = count.fetch_add(1, Ordering::Relaxed);
            pool.execute(move || {
                work();
                let _ = count.fetch_sub(1, Ordering::Relaxed);
            });
        }

        while count.load(Ordering::Relaxed) != 0 {}
    })
}

#[criterion]
fn threadpool_bench_atomic_c1_w1(b: &mut Bencher) {
    threadpool_bench_atomic(b, 1, 1)
}

#[criterion]
fn threadpool_bench_atomic_c2_w1(b: &mut Bencher) {
    threadpool_bench_atomic(b, 2, 1)
}

#[criterion]
fn threadpool_bench_atomic_c2_w2(b: &mut Bencher) {
    threadpool_bench_atomic(b, 2, 2)
}

#[criterion]
fn threadpool_bench_atomic_c1_w2(b: &mut Bencher) {
    threadpool_bench_atomic(b, 1, 2)
}

#[criterion]
fn threadpool_bench_atomic_c20_w4(b: &mut Bencher) {
    threadpool_bench_atomic(b, 1, 2)
}
