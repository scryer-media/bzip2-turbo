use super::{OUTPUT_CHUNK, OUTPUT_OWNERS, OutputPool, Worker, WorkerMessage};
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
        assert!(sender.try_send(WorkerMessage::Finished).is_ok());
    }
    let (ready, started) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        ready.send(()).unwrap();
        assert!(sender.send(WorkerMessage::Finished).is_err());
    });
    started.recv().unwrap();
    drop_with_deadline(Worker {
        jobs: Some(jobs),
        receiver: Some(receiver),
        returned: Some(returned),
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
