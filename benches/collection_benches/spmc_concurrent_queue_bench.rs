extern crate cosmo;
extern crate test;

use self::test::Bencher;
use std::sync::{Arc, Barrier};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Instant;
use self::cosmo::collection::ConcurrentQueue;
use self::cosmo::collection::SpmcConcurrentQueue;

#[bench]
fn offer_poll(b: &mut Bencher) {
    let q = SpmcConcurrentQueue::<u64>::with_capacity(1024);
    b.iter(|| {
        q.offer(10);
        test::black_box(q.poll());
    });
}

#[bench]
fn throughput_one_consumer(b: &mut Bencher) {
    const REPETITIONS: u64 = 10_000_000;
    let q = SpmcConcurrentQueue::<u64>::with_capacity(1024);
    b.iter(|| {
        let barrier = Arc::new(Barrier::new(2));
        let pc = barrier.clone();
        let pq = q.clone();
        thread::spawn(move|| {
            pc.wait();
            for i in 0..REPETITIONS {
                while pq.offer(i).is_some() {
                    thread::yield_now();
                }
            }
        });

        barrier.wait();

        let start = Instant::now();

        for _ in 0..REPETITIONS as u64 {
            let mut opt: Option<u64>;
            while {
                opt = q.poll();
                opt.is_none()
            } {
                thread::yield_now();
            }
            test::black_box(opt);
        }

        let end = start.elapsed();
        let duration = end.as_secs() as f64 + end.subsec_nanos() as f64 / 1000_000_000.0;
        let ops = REPETITIONS as f64 / duration;
        println!("SPMC Queue - ops/sec={}", ops, op);
    });
}

#[bench]
fn throughput_one_producer_two_consumers(b: &mut Bencher) {
    const REPETITIONS: usize = 1 * 1000 * 1000;
    b.iter(|| {
        let q = SpmcConcurrentQueue::<usize>::with_capacity(1024);
        let barrier = Arc::new(Barrier::new(3));
        let done = Arc::new(AtomicBool::new(false));
        let mut consumers = Vec::<thread::JoinHandle<()>>::with_capacity(2);
        for _ in 0..2 {
            let barrier = barrier.clone();
            let q = q.clone();
            let done= done.clone();
            consumers.push(thread::spawn(move|| {
                barrier.wait();
                loop {
                    match q.poll() {
                        Some(_) => continue,
                        None => {
                            if done.load(Ordering::Acquire) && q.is_empty() {
                                break;
                            }
                            thread::yield_now();
                        }
                    }
                }
            }));
        }

        barrier.wait();

        let start = PreciseTime::now();

        for i in 0..REPETITIONS {
            while q.offer(i).is_some() {
                thread::yield_now();
            }
        }
        done.store(true, Ordering::Release);
        for c in consumers { c.join().unwrap(); }

        let end = PreciseTime::now();
        let duration = start.to(end).num_nanoseconds().unwrap() as u64;
        let ops = (REPETITIONS as u64 * 1000_000_000) / duration;
        let op = duration as f64 / REPETITIONS as f64; 
        println!("SPMC Queue - ops/sec={} - ns/op={:.3}", ops, op);
    })  
}

