use super::{
    COLUMN_OWNERS, Decoder, OUTPUT_CHUNK, OUTPUT_OWNERS, OutputPool, Worker, WorkerMessage,
};
use std::sync::mpsc::{self, TrySendError};
use std::time::Duration;

#[test]
fn output_owners_are_bounded_and_reused_without_reallocation() {
    let (returned, receiver) = mpsc::sync_channel(OUTPUT_OWNERS);
    let mut pool = OutputPool {
        returned: receiver,
        allocated: 0,
    };
    let mut owners: Vec<_> = (0..OUTPUT_OWNERS).map(|_| pool.take().unwrap()).collect();
    assert_eq!(pool.allocated, OUTPUT_OWNERS);
    for _ in 0..64 {
        let mut bytes = owners.pop().unwrap();
        let address = bytes.as_ptr();
        bytes.truncate(7);
        returned.try_send(bytes).unwrap();
        let bytes = pool.take().unwrap();
        assert_eq!(bytes.as_ptr(), address);
        assert_eq!(bytes.len(), OUTPUT_CHUNK);
        assert_eq!(pool.allocated, OUTPUT_OWNERS);
        owners.push(bytes);
    }
    drop(returned);
    assert!(pool.take().is_none());
}

fn drop_with_deadline(worker: Worker) {
    let (finished, done) = mpsc::channel();
    std::thread::spawn(move || {
        drop(worker);
        finished.send(()).unwrap();
    });
    done.recv_timeout(Duration::from_secs(5)).unwrap();
}

#[test]
fn worker_drop_disconnects_a_blocked_output_send_before_join() {
    let (jobs, _blocks) = mpsc::sync_channel(1);
    let (sender, receiver) = mpsc::sync_channel(16);
    let (returned, _buffers) = mpsc::sync_channel(OUTPUT_OWNERS);
    for _ in 0..16 {
        assert!(
            sender
                .try_send(WorkerMessage::Finished(Default::default()))
                .is_ok()
        );
    }
    let (ready, started) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        ready.send(()).unwrap();
        assert!(
            sender
                .send(WorkerMessage::Finished(Default::default()))
                .is_err()
        );
    });
    started.recv().unwrap();
    drop_with_deadline(Worker {
        jobs: Some(jobs),
        receiver: Some(receiver),
        returned: Some(returned),
        cancelled: Default::default(),
        handle: Some(handle),
    });
}

#[test]
fn worker_drop_disconnects_a_blocked_owner_receive_before_join() {
    let (jobs, _blocks) = mpsc::sync_channel(1);
    let (_sender, receiver) = mpsc::sync_channel(16);
    let (returned, buffers) = mpsc::sync_channel(OUTPUT_OWNERS);
    let (ready, started) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let mut pool = OutputPool {
            returned: buffers,
            allocated: OUTPUT_OWNERS,
        };
        ready.send(()).unwrap();
        assert!(pool.take().is_none());
    });
    started.recv().unwrap();
    drop_with_deadline(Worker {
        jobs: Some(jobs),
        receiver: Some(receiver),
        returned: Some(returned),
        cancelled: Default::default(),
        handle: Some(handle),
    });
}

#[test]
fn returning_an_owner_never_waits_for_the_worker() {
    let (jobs, _blocks) = mpsc::sync_channel(1);
    let (_sender, receiver) = mpsc::sync_channel(16);
    let (returned, _buffers) = mpsc::sync_channel(OUTPUT_OWNERS);
    // A synthetic ownership violation must fail immediately in debug builds,
    // and must never wait behind a worker blocked on output delivery.
    for _ in 0..OUTPUT_OWNERS {
        returned.try_send(Vec::with_capacity(OUTPUT_CHUNK)).unwrap();
    }
    assert!(matches!(
        returned.try_send(Vec::new()),
        Err(TrySendError::Full(_))
    ));
    let worker = Worker {
        jobs: Some(jobs),
        receiver: Some(receiver),
        returned: Some(returned),
        cancelled: Default::default(),
        handle: None,
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        worker.recycle(Vec::with_capacity(OUTPUT_CHUNK));
    }));
    assert_eq!(result.is_err(), cfg!(debug_assertions));
    drop_with_deadline(worker);
}

#[test]
fn oversized_allocation_capacity_keeps_its_output_owner_credit() {
    let (jobs, _blocks) = mpsc::sync_channel(1);
    let (_sender, receiver) = mpsc::sync_channel(16);
    let (returned, buffers) = mpsc::sync_channel(OUTPUT_OWNERS);
    let worker = Worker {
        jobs: Some(jobs),
        receiver: Some(receiver),
        returned: Some(returned),
        cancelled: Default::default(),
        handle: None,
    };
    let bytes = Vec::with_capacity(OUTPUT_CHUNK + 1);
    let address = bytes.as_ptr();
    worker.recycle(bytes);
    let mut pool = OutputPool {
        returned: buffers,
        allocated: OUTPUT_OWNERS,
    };
    let bytes = pool.take().unwrap();
    assert_eq!(bytes.as_ptr(), address);
    assert!(bytes.capacity() > OUTPUT_CHUNK);
    assert_eq!(bytes.len(), OUTPUT_CHUNK);
    assert_eq!(pool.allocated, OUTPUT_OWNERS);
    drop_with_deadline(worker);
}

#[test]
fn a_cancelled_worker_never_starts_a_queued_block() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    let (encoded, _) = members(&[1], 1000);
    let mut parser = super::Parser::new(encoded.as_slice());
    let block = parser.next().unwrap().expect("one block");
    let observed = Arc::new(AtomicUsize::new(0));
    let state = Arc::clone(&observed);
    let callback: super::OutputCallback = Arc::new(Mutex::new(Box::new(move |bytes: &[u8]| {
        state.fetch_add(bytes.len(), Ordering::SeqCst);
    })));
    let worker = Worker::start(Some(callback), super::Mode::platform()).unwrap();
    // Drop sets the flag before closing the queue; set it first here so the
    // worker sees it on the buffered block whether or not drop has run yet.
    worker.cancelled.store(true, Ordering::Release);
    worker.submit(block).unwrap();
    // Give an uncancelled worker time to reach the callback before drop
    // closes the owner queue, which would also stop it.
    std::thread::sleep(Duration::from_millis(50));
    drop_with_deadline(worker);
    assert_eq!(observed.load(Ordering::SeqCst), 0);
}

fn members(levels: &[u32], length: usize) -> (Vec<u8>, Vec<u8>) {
    use std::io::Write;
    let mut plain = Vec::new();
    let mut encoded = Vec::new();
    for (member, &level) in levels.iter().enumerate() {
        let data: Vec<u8> = (0..length)
            .map(|i| ((i * 131 + member * 7) ^ (i >> 9)) as u8)
            .collect();
        let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::new(level));
        encoder.write_all(&data).unwrap();
        encoded.extend(encoder.finish().unwrap());
        plain.extend(data);
    }
    (encoded, plain)
}

#[test]
fn last_column_owners_never_exceed_their_bound_and_are_reused() {
    use std::io::Read;
    // Many level-1 blocks, then a level-9 member whose limit regrows the
    // recycled owners, then level 1 again.
    let (encoded, plain) = members(&[1, 9, 1], 650_003);
    for threads in [1, 2, 4] {
        for low_memory in [false, true] {
            for width in [1, 4093, 1 << 20] {
                let mut decoder = if low_memory {
                    Decoder::with_low_memory(encoded.as_slice(), threads)
                } else {
                    Decoder::with_threads(encoded.as_slice(), threads)
                };
                let mut decoded = Vec::with_capacity(plain.len());
                let mut buffer = vec![0; width];
                loop {
                    let n = decoder.read(&mut buffer).unwrap();
                    let owners = decoder.parser.columns.owners;
                    assert!(owners <= if threads == 1 { 1 } else { COLUMN_OWNERS });
                    assert!(owners >= decoder.parser.columns.spare.len());
                    if n == 0 {
                        break;
                    }
                    decoded.extend_from_slice(&buffer[..n]);
                }
                assert!(decoded == plain, "threads {threads}, width {width}");
                // Every owner is back in the pool once the stream has ended.
                let pool = &decoder.parser.columns;
                assert_eq!(pool.spare.len(), pool.owners);
                // Run-ahead parses the second block while the first is in flight.
                assert_eq!(pool.owners, if threads == 1 { 1 } else { COLUMN_OWNERS });
            }
        }
    }
}
