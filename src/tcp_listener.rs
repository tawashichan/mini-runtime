use std::io;
use std::net::TcpListener;
use std::net::ToSocketAddrs;
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
pub struct AsyncTcpListener(mio::net::TcpListener);

impl AsyncTcpListener {
    pub fn bind(addr: std::net::SocketAddr) -> Result<AsyncTcpListener, io::Error> {
        let mut inner = mio::net::TcpListener::bind(addr)?;
        let fd = inner.as_raw_fd();
        REACTOR.with(|reactor| reactor.register_source(&mut inner, mio::Token(fd as usize)));
        Ok(AsyncTcpListener(inner))
    }

    pub fn incoming(self) -> Incoming {
        Incoming(self.0)
    }
}

pub struct Incoming(mio::net::TcpListener);

impl Stream for Incoming {
    type Item = AsyncTcpStream;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Option<Self::Item>> {
        let fd = self.0.as_raw_fd();
        let waker = ctx.waker();

        match self.0.accept() {
            Ok((conn, _)) => {
                let stream = AsyncTcpStream::from_std(conn);
                Poll::Ready(Some(stream))
            }
            Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                REACTOR
                    .with(|reactor| reactor.register_entry(mio::Token(fd as usize), waker.clone()));
                Poll::Pending
            }
            Err(err) => panic!("error {:?}", err),
        }
    }
}
