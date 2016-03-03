#![allow(deprecated)]

extern crate threadpool;

use threadpool::ThreadPool;
use std::sync::mpsc::channel;
use std::sync::{Arc, Barrier};
use std::thread::sleep_ms;

const TEST_TASKS: usize = 4;

#[test]
fn test_set_threads_increasing() {
    let new_thread_amount = 6;
    let mut pool = ThreadPool::new(TEST_TASKS);
    for _ in 0..TEST_TASKS {
        pool.execute(move || {
            loop {
                sleep_ms(10000)
            }
        });
    }
    pool.set_threads(new_thread_amount);
    for _ in 0..(new_thread_amount - TEST_TASKS) {
        pool.execute(move || {
            loop {
                sleep_ms(10000)
            }
        });
    }
    sleep_ms(1000);
    assert_eq!(pool.active_count(), new_thread_amount);
}

#[test]
fn test_set_threads_decreasing() {
    let new_thread_amount = 2;
    let mut pool = ThreadPool::new(TEST_TASKS);
    for _ in 0..TEST_TASKS {
        pool.execute(move || {
            1 + 1;
        });
    }
    pool.set_threads(new_thread_amount);
    for _ in 0..new_thread_amount {
        pool.execute(move || {
            loop {
                sleep_ms(10000)
            }
        });
    }
    sleep_ms(1000);
    assert_eq!(pool.active_count(), new_thread_amount);
}

#[test]
fn test_active_count() {
    let pool = ThreadPool::new(TEST_TASKS);
    for _ in 0..TEST_TASKS {
        pool.execute(move|| {
            loop {
                sleep_ms(10000);
            }
        });
    }
    sleep_ms(1000);
    let active_count = pool.active_count();
    assert_eq!(active_count, TEST_TASKS);
    let initialized_count = pool.max_count();
    assert_eq!(initialized_count, TEST_TASKS);
}

#[test]
fn test_works() {
    let pool = ThreadPool::new(TEST_TASKS);

    let (tx, rx) = channel();
    for _ in 0..TEST_TASKS {
        let tx = tx.clone();
        pool.execute(move|| {
            tx.send(1).unwrap();
        });
    }

    assert_eq!(rx.iter().take(TEST_TASKS).fold(0, |a, b| a + b), TEST_TASKS);
}

#[test]
#[should_panic]
fn test_zero_tasks_panic() {
    ThreadPool::new(0);
}

#[test]
fn test_recovery_from_subtask_panic() {
    let pool = ThreadPool::new(TEST_TASKS);

    // Panic all the existing threads.
    for _ in 0..TEST_TASKS {
        pool.execute(move|| -> () { panic!() });
    }

    // Ensure new threads were spawned to compensate.
    let (tx, rx) = channel();
    for _ in 0..TEST_TASKS {
        let tx = tx.clone();
        pool.execute(move|| {
            tx.send(1).unwrap();
        });
    }

    assert_eq!(rx.iter().take(TEST_TASKS).fold(0, |a, b| a + b), TEST_TASKS);
}

#[test]
fn test_should_not_panic_on_drop_if_subtasks_panic_after_drop() {

    let pool = ThreadPool::new(TEST_TASKS);
    let waiter = Arc::new(Barrier::new(TEST_TASKS + 1));

    // Panic all the existing threads in a bit.
    for _ in 0..TEST_TASKS {
        let waiter = waiter.clone();
        pool.execute(move|| {
            waiter.wait();
            panic!();
        });
    }

    drop(pool);

    // Kick off the failure.
    waiter.wait();
}
