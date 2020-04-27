use std::io;
use std::os::unix::io::AsRawFd;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use mio;

use futures_core::Stream;

use crate::tcp_stream::AsyncTcpStream;
use crate::REACTOR;

// AsyncTcpListener just wraps std tcp listener
#[derive(Debug)]
pub struct AsyncTcpListener(ListenerWatcher);

#[derive(Debug)]
struct ListenerWatcher {
    mio_token: mio::Token,
    listener: mio::net::TcpListener,
}

impl ListenerWatcher {
    fn new(token: mio::Token, listener: mio::net::TcpListener) -> ListenerWatcher {
        let mut listener = listener;
        REACTOR
            .with(|reactor| reactor.register_source(&mut listener, token, mio::Interest::READABLE));
        ListenerWatcher {
            mio_token: token,
            listener: listener,
        }
    }
}

impl AsyncTcpListener {
    pub fn bind(addr: std::net::SocketAddr) -> Result<AsyncTcpListener, io::Error> {
        let inner = mio::net::TcpListener::bind(addr)?;
        let fd = inner.as_raw_fd();
        let watcher = ListenerWatcher::new(mio::Token(fd as usize), inner);
        Ok(AsyncTcpListener(watcher))
    }

    pub fn incoming(self) -> Incoming {
        Incoming(self)
    }
}

pub struct Incoming(AsyncTcpListener);

impl Stream for Incoming {
    type Item = AsyncTcpStream;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Option<Self::Item>> {
        let waker = ctx.waker();

        match (self.0).0.listener.accept() {
            Ok((conn, _)) => {
                let stream = AsyncTcpStream::from_mio(conn);
                Poll::Ready(Some(stream))
            }
            Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                REACTOR.with(|reactor| reactor.register_entry((self.0).0.mio_token, waker.clone()));
                Poll::Pending
            }
            Err(err) => panic!("error {:?}", err),
        }
    }
}
