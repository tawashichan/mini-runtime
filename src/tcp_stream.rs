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
pub struct AsyncTcpStream(mio::net::TcpStream);

impl AsyncTcpStream {
    pub fn connect(addr: std::net::SocketAddr) -> Result<AsyncTcpStream, io::Error> {
        let mut inner = mio::net::TcpStream::connect(addr)?;
        let fd = inner.as_raw_fd();
        REACTOR.with(|reactor| reactor.register_source(&mut inner, mio::Token(fd as usize)));
        Ok(AsyncTcpStream(inner))
    }

    pub fn from_std(conn: mio::net::TcpStream) -> AsyncTcpStream {
        AsyncTcpStream(conn)
    }
}

impl Drop for AsyncTcpStream {
    fn drop(&mut self) {
        /*REACTOR.with(|reactor| {
            let fd = self.0.as_raw_fd();
            reactor.remove_read_interest(fd);
            reactor.remove_write_interest(fd);
        });*/
    }
}

impl AsyncRead for AsyncTcpStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        ctx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, Error>> {
        let fd = self.0.as_raw_fd();
        let waker = ctx.waker();

        match self.0.read(buf) {
            Ok(len) => Poll::Ready(Ok(len)),
            Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                REACTOR
                    .with(|reactor| reactor.register_entry(mio::Token(fd as usize), waker.clone()));

                Poll::Pending
            }
            Err(err) => panic!("error {:?}", err),
        }
    }
}

impl AsyncWrite for AsyncTcpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        ctx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, Error>> {
        let fd = self.0.as_raw_fd();
        let waker = ctx.waker();

        match self.0.write(buf) {
            Ok(len) => Poll::Ready(Ok(len)),
            Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                REACTOR
                    .with(|reactor| reactor.register_entry(mio::Token(fd as usize), waker.clone()));
                Poll::Pending
            }
            Err(err) => panic!("error {:?}", err),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _lw: &mut Context) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _lw: &mut Context) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }
}
