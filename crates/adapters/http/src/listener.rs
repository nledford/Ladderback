//! The daemon's listener: it accepts each connection through the spawn gate
//! (technical design §16, decision 66).
//!
//! A child inherits every descriptor of its parent that is not close-on-exec
//! when it starts. Where the platform has no `accept4` (macOS), a connection
//! is accepted in two steps, `accept` and then `fcntl` for close-on-exec, so
//! a worker, a hook or a password command that another thread starts in
//! between holds the browser's connection open for as long as it runs. An
//! accept that holds the gate cannot overlap a start (decision 58).
//!
//! An accept takes the gate where it is free at once, on the runtime's
//! thread. While a start holds it, as one whose `exec` stalls can for long,
//! the accept waits for it on tokio's blocking pool and accepts there, never
//! on one of the runtime's threads, which serve every other request
//! meanwhile.

use std::io;
use std::net::{SocketAddr, TcpListener as StdListener};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::net::{TcpListener, TcpStream};

/// How a connection is taken off the listening socket: tokio's own accept,
/// which takes one only once the socket is ready.
type Accept = fn(&TcpListener, &mut Context<'_>) -> Poll<io::Result<(TcpStream, SocketAddr)>>;

/// Whether the platform makes a socket, and accepts a connection, in two
/// steps, the descriptor and then `fcntl` for close-on-exec: it has no
/// `SOCK_CLOEXEC` and no `accept4`. Only there does the listener take the
/// spawn gate.
const IN_TWO_STEPS: bool = cfg!(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd"
)));

/// A TCP listener whose every accept holds the spawn gate, for
/// `axum::serve`.
pub struct GatedListener {
    listener: Arc<TcpListener>,
    accept: Accept,
}

impl GatedListener {
    /// Listens on `address`. Call it within a tokio runtime.
    ///
    /// # Errors
    ///
    /// The address can't be listened on.
    pub async fn bind(address: SocketAddr) -> io::Result<Self> {
        let _ = (address, AFTER_FAILURE, Self::with_accept, IN_TWO_STEPS);
        todo!("#283: listen through the gate")
    }

    fn with_accept(listener: StdListener, accept: Accept) -> io::Result<Self> {
        let _ = (listener, accept, Arc::<()>::default);
        todo!("#283: a listener that accepts through the gate")
    }

    /// Where it listens.
    ///
    /// # Errors
    ///
    /// The system can't tell.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// The next connection, accepted while the spawn gate is held.
    ///
    /// # Errors
    ///
    /// The accept failed, or the runtime shut down while it waited for the
    /// gate.
    pub async fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        let _ = (self.accept, is_lost_connection);
        todo!("#283: accept through the gate")
    }
}

impl axum::serve::Listener for GatedListener {
    type Io = TcpStream;
    type Addr = SocketAddr;

    /// The next connection. A connection its client gave up on before it
    /// was accepted is passed over at once; after any other failure, such as
    /// a daemon out of descriptors, the accept is tried again
    /// [`AFTER_FAILURE`] later, as `axum::serve` does with its own listener.
    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        todo!("#283: serve through the gate")
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Self::local_addr(self)
    }
}

/// How long the server waits to accept again after an accept failed for a
/// reason other than its client.
const AFTER_FAILURE: Duration = Duration::from_secs(1);

/// Whether an accept failed for a connection its client gave up on, not for
/// a reason of the daemon's own.
fn is_lost_connection(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
    )
}

// `nix`, and the spawn gate's test kit, are Unix only. Two attributes, not
// `all(test, unix)`: clippy lets tests unwrap only under a plain `cfg(test)`.
#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::net::Ipv4Addr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    use axum::serve::Listener;

    use super::*;

    fn loopback() -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0))
    }

    fn listening(accept: Accept) -> GatedListener {
        GatedListener::with_accept(StdListener::bind(loopback()).unwrap(), accept).unwrap()
    }

    /// The tests of the gate, compiled only where the platform accepts in two
    /// steps (the platforms [`IN_TWO_STEPS`] names): elsewhere the server
    /// doesn't take the gate, so they could prove nothing, and a test that
    /// can't fail there must not count as a passing one.
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "freebsd")))]
    mod two_steps {
        use std::future::poll_fn;
        use std::os::fd::{AsRawFd, RawFd};
        use std::pin::Pin;
        use std::process::Command;
        use std::sync::mpsc;
        use std::sync::{Mutex, PoisonError};
        use std::thread;

        use testkit::inheritable::{INHERITED, OPEN_FOR, stalled_start, start_in_flight};

        use super::*;

        /// Where `held_open` says which descriptor it holds open.
        static HELD: Mutex<Option<mpsc::Sender<RawFd>>> = Mutex::new(None);

        /// Where `held_open_late` says which descriptor it holds open.
        static HELD_LATE: Mutex<Option<mpsc::Sender<RawFd>>> = Mutex::new(None);

        /// Accepts as tokio does, then keeps the connection open without
        /// close-on-exec for [`OPEN_FOR`], as an accept does between its two
        /// steps where the platform has no `accept4`: in a copy that `dup` made,
        /// which is never close-on-exec. Says which descriptor through `held`.
        fn held_open_through(
            held: &Mutex<Option<mpsc::Sender<RawFd>>>,
            listener: &TcpListener,
            cx: &mut Context<'_>,
        ) -> Poll<io::Result<(TcpStream, SocketAddr)>> {
            let accepted = listener.poll_accept(cx);
            if let Poll::Ready(Ok((stream, _))) = &accepted {
                let copy = nix::unistd::dup(stream).expect("a copy of the connection");
                let held = held.lock().unwrap_or_else(PoisonError::into_inner).take();
                if let Some(held) = held {
                    let _ = held.send(copy.as_raw_fd());
                }
                thread::sleep(OPEN_FOR);
            }
            accepted
        }

        fn held_open(
            listener: &TcpListener,
            cx: &mut Context<'_>,
        ) -> Poll<io::Result<(TcpStream, SocketAddr)>> {
            held_open_through(&HELD, listener, cx)
        }

        fn held_open_late(
            listener: &TcpListener,
            cx: &mut Context<'_>,
        ) -> Poll<io::Result<(TcpStream, SocketAddr)>> {
            held_open_through(&HELD_LATE, listener, cx)
        }

        /// What a child started now, the way a worker, a hook or a password
        /// command is, inherits of descriptor `number`: its number, or nothing.
        async fn inherited_by_a_child(number: RawFd) -> String {
            let number = number.to_string();
            let child = spawn_gate::hold_blocking(move || {
                Command::new("/bin/sh")
                    .args(["-c", INHERITED, "sh", &number])
                    .output()
            })
            .await
            .unwrap()
            .unwrap();
            assert!(child.status.success());
            String::from_utf8_lossy(&child.stdout).into_owned()
        }

        /// Polls `future` once: whether it is still pending.
        async fn pending_after_one_poll<F: Future + Unpin>(future: &mut F) -> bool {
            poll_fn(|cx| Poll::Ready(Pin::new(&mut *future).poll(cx).is_pending())).await
        }

        /// A child that starts while the server accepts a connection inherits
        /// none of it: the accept holds the spawn gate, which every start of a
        /// worker, a hook or a password command takes (#283). Here the accept
        /// keeps the connection open without close-on-exec a while, as it is for
        /// a moment where the platform has no `accept4`, and a child starts the
        /// way they do meanwhile.
        #[tokio::test(flavor = "multi_thread")]
        async fn a_child_started_during_an_accept_inherits_no_connection() {
            let (held, holding) = mpsc::channel();
            *HELD.lock().unwrap() = Some(held);
            let listener = listening(held_open);
            let address = listener.local_addr().unwrap();
            let accepting = tokio::spawn(async move { listener.accept().await.map(|_| ()) });
            let _client = testkit::net::connect(address).unwrap();
            let number = holding
                .recv_timeout(Duration::from_secs(5))
                .expect("the accept took the connection");
            let inherited = inherited_by_a_child(number).await;
            accepting.await.unwrap().unwrap();
            assert_eq!(
                inherited, "",
                "the child inherited the connection being accepted"
            );
        }

        /// An accept that waited for the gate on the blocking pool accepts there
        /// while it still holds the gate, not once it has let go: a child that
        /// starts meanwhile inherits none of the connection. Here a start holds
        /// the gate when the accept begins, and lets go once it waits.
        #[tokio::test(flavor = "multi_thread")]
        async fn a_child_started_during_an_accept_that_waited_for_the_gate_inherits_no_connection()
        {
            let (held, holding) = mpsc::channel();
            *HELD_LATE.lock().unwrap() = Some(held);
            let listener = listening(held_open_late);
            let _client = testkit::net::connect(listener.local_addr().unwrap()).unwrap();
            let (release, stalled) = stalled_start(Duration::from_secs(5));
            let mut accepting = Box::pin(async move { listener.accept().await.map(|_| ()) });
            assert!(
                pending_after_one_poll(&mut accepting).await,
                "the accept went through a gate a start held"
            );
            let accepting = tokio::spawn(accepting);
            release.send(()).unwrap();
            let number = holding
                .recv_timeout(Duration::from_secs(5))
                .expect("the accept took the connection");
            let inherited = inherited_by_a_child(number).await;
            accepting.await.unwrap().unwrap();
            assert!(stalled.join().unwrap());
            assert_eq!(
                inherited, "",
                "the child inherited the connection being accepted"
            );
        }

        /// The listening socket is made through the gate too, where there is no
        /// `SOCK_CLOEXEC` in two steps as well: a child that held it would keep
        /// the address taken after the daemon stopped. Here a start is in
        /// flight, and the listener is made only once it has let go.
        #[tokio::test]
        async fn the_listener_is_made_while_no_start_is_in_flight() {
            let start = start_in_flight(Duration::from_millis(300));
            let listener = GatedListener::bind(loopback()).await;
            let made = Instant::now();
            let let_go = start.join().unwrap();
            assert!(listener.is_ok());
            assert!(
                made >= let_go,
                "the listener was made while a start held the gate"
            );
        }

        /// An accept waits for the gate off the runtime's threads: a start whose
        /// `exec` stalls holds the gate, and an accept that waited for it on a
        /// runtime thread would stop that thread's other tasks, other requests
        /// among them. Here a task of the same one-thread runtime lets the gate
        /// go a while after the client connects, which it can only while the
        /// accept waits elsewhere.
        #[tokio::test]
        async fn an_accept_waits_for_the_gate_off_the_runtime() {
            let listener = GatedListener::bind(loopback()).await.unwrap();
            let address = listener.local_addr().unwrap();
            let (release, stalled) = stalled_start(Duration::from_secs(2));
            let _client = testkit::net::connect(address).unwrap();
            let let_go = async {
                tokio::time::sleep(Duration::from_millis(200)).await;
                let _ = release.send(());
            };
            let (accepted, ()) = tokio::join!(listener.accept(), let_go);
            let (_, peer) = accepted.unwrap();
            assert!(
                stalled.join().unwrap(),
                "the accept waited for the gate on the runtime's thread"
            );
            assert!(peer.ip().is_loopback());
        }

        /// Where `pending_told` says it found no connection.
        static FOUND_NONE: Mutex<Option<mpsc::Sender<()>>> = Mutex::new(None);

        /// Accepts as tokio does, and says the first time it finds no
        /// connection.
        fn pending_told(
            listener: &TcpListener,
            cx: &mut Context<'_>,
        ) -> Poll<io::Result<(TcpStream, SocketAddr)>> {
            let accepted = listener.poll_accept(cx);
            if accepted.is_pending() {
                let told = FOUND_NONE
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take();
                if let Some(told) = told {
                    let _ = told.send(());
                }
            }
            accepted
        }

        /// An accept that found no connection while it held the gate on the
        /// blocking pool waits for the next with its own task: a client that
        /// connects only then is still accepted. Here a start holds the gate
        /// when the accept begins, and the client connects once the accept has
        /// looked on the blocking pool and found none.
        #[tokio::test]
        async fn an_accept_that_waited_for_the_gate_takes_a_later_connection() {
            let (told, found_none) = mpsc::channel();
            *FOUND_NONE.lock().unwrap() = Some(told);
            let listener = listening(pending_told);
            let address = listener.local_addr().unwrap();
            let (release, stalled) = stalled_start(Duration::from_secs(5));
            let mut accepting = std::pin::pin!(listener.accept());
            assert!(
                pending_after_one_poll(&mut accepting).await,
                "the accept went through a gate a start held"
            );
            release.send(()).unwrap();
            found_none
                .recv_timeout(Duration::from_secs(5))
                .expect("the accept looked for a connection once the gate was free");
            let _client = tokio::net::TcpStream::connect(address).await.unwrap();
            let accepted = tokio::time::timeout(Duration::from_secs(5), accepting)
                .await
                .expect("the accept missed the connection made after it waited for the gate");
            assert!(stalled.join().unwrap());
            assert!(accepted.unwrap().1.ip().is_loopback());
        }
    }

    /// Whether `aborted_once` has failed yet.
    static ABORTED: AtomicBool = AtomicBool::new(false);

    /// Fails its first accept as one fails for a connection that its client
    /// reset before it was taken, then accepts.
    fn aborted_once(
        listener: &TcpListener,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<(TcpStream, SocketAddr)>> {
        if !ABORTED.swap(true, Ordering::SeqCst) {
            return Poll::Ready(Err(io::ErrorKind::ConnectionAborted.into()));
        }
        listener.poll_accept(cx)
    }

    /// A connection its client gave up on before the server took it holds
    /// up no other: the server accepts the next at once.
    #[tokio::test]
    async fn the_server_accepts_on_at_once_after_a_lost_connection() {
        let mut listener = listening(aborted_once);
        let _client = testkit::net::connect(listener.local_addr().unwrap()).unwrap();
        let accepted =
            tokio::time::timeout(AFTER_FAILURE / 2, Listener::accept(&mut listener)).await;
        assert!(ABORTED.load(Ordering::SeqCst));
        assert!(
            accepted.is_ok(),
            "the server paused after a connection its client gave up on"
        );
    }

    /// Whether `failed_once` has failed yet.
    static FAILED: AtomicBool = AtomicBool::new(false);

    /// Fails its first accept as one fails in a daemon out of descriptors,
    /// then accepts.
    fn failed_once(
        listener: &TcpListener,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<(TcpStream, SocketAddr)>> {
        if !FAILED.swap(true, Ordering::SeqCst) {
            return Poll::Ready(Err(nix::errno::Errno::EMFILE.into()));
        }
        listener.poll_accept(cx)
    }

    /// After an accept failed for a reason of the daemon's own, such as no
    /// descriptor left, the server tries again a while later, not at once:
    /// it would spin until a descriptor came free.
    #[tokio::test]
    async fn the_server_pauses_after_an_accept_failed() {
        let mut listener = listening(failed_once);
        let _client = testkit::net::connect(listener.local_addr().unwrap()).unwrap();
        let began = Instant::now();
        let accepted =
            tokio::time::timeout(AFTER_FAILURE * 3, Listener::accept(&mut listener)).await;
        assert!(accepted.is_ok(), "the server stopped accepting");
        assert!(FAILED.load(Ordering::SeqCst));
        assert!(
            began.elapsed() >= AFTER_FAILURE,
            "the server tried again at once after an accept failed"
        );
    }
}
