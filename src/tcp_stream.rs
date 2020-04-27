use std::io::Error;
use std::io::{self, Read, Write};
use std::os::unix::io::AsRawFd;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use mio;

use futures_io::{AsyncRead, AsyncWrite};

use crate::REACTOR;

// AsyncTcpStream just wraps std tcp stream
#[derive(Debug)]
pub struct AsyncTcpStream(StreamWatcher);

#[derive(Debug)]
struct StreamWatcher {
    mio_token: mio::Token,
    stream: mio::net::TcpStream,
}

impl StreamWatcher {
    fn new(token: mio::Token, stream: mio::net::TcpStream) -> StreamWatcher {
        let mut stream = stream;
        REACTOR.with(|reactor| {
            reactor.register_source(
                &mut stream,
                token,
                mio::Interest::READABLE | mio::Interest::WRITABLE,
            )
        });
        StreamWatcher {
            mio_token: token,
            stream: stream,
        }
    }
}

impl AsyncTcpStream {
    pub fn connect(addr: std::net::SocketAddr) -> Result<AsyncTcpStream, io::Error> {
        //let inner = mio::net::TcpStream::connect(addr)?;

        let conn = std::net::TcpStream::connect(addr)?;
        conn.set_nonblocking(true)?;
        let inner = mio::net::TcpStream::from_std(conn);
        let fd = inner.as_raw_fd();
        let watcher = StreamWatcher::new(mio::Token(fd as usize), inner);
        Ok(AsyncTcpStream(watcher))
    }

    pub fn from_mio(conn: mio::net::TcpStream) -> AsyncTcpStream {
        let fd = conn.as_raw_fd();
        let watcher = StreamWatcher::new(mio::Token(fd as usize), conn);
        AsyncTcpStream(watcher)
    }
}

impl Drop for AsyncTcpStream {
    fn drop(&mut self) {
        REACTOR.with(|reactor| {
            /*let fd = self.0.as_raw_fd();
            let token = mio::Token(fd as usize);
            reactor.deregister_entry(&token);
            reactor.deregister_source(&mut self.0);*/
        });
    }
}

impl AsyncRead for AsyncTcpStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        ctx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, Error>> {
        let waker = ctx.waker();

        match (self.0).stream.read(buf) {
            Ok(len) => Poll::Ready(Ok(len)),
            Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                REACTOR.with(|reactor| reactor.register_entry(self.0.mio_token, waker.clone()));
                Poll::Pending
            }
            Err(err) => Poll::Ready(Err(err)),
        }
    }
}

impl AsyncWrite for AsyncTcpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        ctx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, Error>> {
        let waker = ctx.waker();

        match (self.0).stream.write(buf) {
            Ok(len) => Poll::Ready(Ok(len)),
            Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                REACTOR.with(|reactor| reactor.register_entry(self.0.mio_token, waker.clone()));
                Poll::Pending
            }
            Err(err) => Poll::Ready(Err(err)),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _lw: &mut Context) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _lw: &mut Context) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }
}
