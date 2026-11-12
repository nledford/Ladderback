//! A TCP connection a test opens to a daemon, that is open when it is
//! returned.
//!
//! `TcpStream::connect` is not that on macOS. A signal that arrives while it
//! waits interrupts the `connect` call (macOS reports `EINTR` even under
//! `SA_RESTART`); the standard library calls `connect` again, macOS answers
//! `EISCONN` for a connection still on its way, and the standard library
//! takes that for success. The stream comes back unconnected: a write fails
//! with `ENOTCONN` ("Socket is not connected"), and a read waits for its
//! timeout and fails with `EAGAIN` (`WouldBlock`). A test process gets such
//! signals once anything in it has started a `tokio::process` child: tokio
//! then handles `SIGCHLD`, which every child of the process sends when it
//! ends. `TcpStream::connect_timeout` waits for the connection with `poll`,
//! which an interruption only restarts, and reads the outcome from the socket.

use std::io;
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

/// How long a connection to a daemon on this machine may take to open. Far
/// longer than one takes; a connection that needs it is a failure anyway.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Opens a TCP connection to `addr`, and returns it once it is open, whatever
/// signals arrive meanwhile. Fails after [`CONNECT_TIMEOUT`].
///
/// # Errors
///
/// When the connection is refused, fails, or is not open within
/// [`CONNECT_TIMEOUT`].
pub fn connect(addr: SocketAddr) -> io::Result<TcpStream> {
    TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
}

#[cfg(all(test, unix))]
mod tests {
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Instant;

    use nix::sys::pthread::{pthread_kill, pthread_self};
    use nix::sys::signal::Signal;
    use tokio::signal::unix::{SignalKind, signal};

    use super::*;

    /// How long a connection that cannot be queued waits, at most, before it
    /// counts as waiting.
    const QUEUED_WITHIN: Duration = Duration::from_millis(200);

    /// A listener whose accept queue is full, so that the next connection
    /// waits until the queue is drained, and the connections that fill it.
    fn full_listener(runtime: &tokio::runtime::Runtime) -> (TcpListener, Vec<TcpStream>) {
        let _entered = runtime.enter();
        let socket = tokio::net::TcpSocket::new_v4().unwrap();
        socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let listener = socket.listen(1).unwrap().into_std().unwrap();
        listener.set_nonblocking(false).unwrap();
        let addr = listener.local_addr().unwrap();
        let mut queued = Vec::new();
        while let Ok(stream) = TcpStream::connect_timeout(&addr, QUEUED_WITHIN) {
            queued.push(stream);
            assert!(queued.len() < 64, "the accept queue never filled");
        }
        (listener, queued)
    }

    #[test]
    fn a_signal_while_connecting_never_yields_a_stream_that_is_not_connected() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        // tokio handles SIGCHLD from here on, as it does once a test starts a
        // `tokio::process` child.
        let _children = runtime.block_on(async { signal(SignalKind::child()).unwrap() });
        let (listener, queued) = full_listener(&runtime);
        let addr = listener.local_addr().unwrap();

        let (started, waiting) = mpsc::channel();
        let connecting = thread::spawn(move || {
            started.send(pthread_self()).unwrap();
            let stream = connect(addr).unwrap();
            // Whether the stream is connected, asked as soon as it returns.
            (stream.peer_addr().map_err(|e| e.kind()), stream)
        });
        let thread = waiting.recv().unwrap();
        // The connection waits for room in the queue; a child ending would
        // signal the process now.
        thread::sleep(QUEUED_WITHIN);
        if connecting.is_finished() {
            crate::skip!("the accept queue did not fill, so the connection never waited");
        }
        pthread_kill(thread, Signal::SIGCHLD).unwrap();
        thread::sleep(QUEUED_WITHIN);
        drop(queued);
        let drained = Instant::now();
        listener.set_nonblocking(true).unwrap();
        while !connecting.is_finished() && drained.elapsed() < CONNECT_TIMEOUT {
            let _ = listener.accept();
            thread::sleep(Duration::from_millis(10));
        }

        let (peer, _stream) = connecting.join().unwrap();
        assert_eq!(
            peer,
            Ok(addr),
            "the stream is connected when it is returned"
        );
    }
}
