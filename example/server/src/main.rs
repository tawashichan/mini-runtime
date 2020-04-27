use futures::io::{AsyncReadExt, AsyncWriteExt};
use futures::stream::StreamExt;
use mini_runtime::tcp_listener;
use mini_runtime::tcp_stream;
use mini_runtime::{run, spawn};

fn main() {
    async fn listen(addr: &str) {
        let addr = addr.parse().unwrap();
        let listener = tcp_listener::AsyncTcpListener::bind(addr).unwrap();
        let mut incoming = listener.incoming();

        while let Some(stream) = incoming.next().await {
            spawn(process(stream));
        }
    }

    async fn process(mut stream: tcp_stream::AsyncTcpStream) {
        let mut buf = vec![0; 10];
        let _ = stream.read_exact(&mut buf).await;
        println!("{}", String::from_utf8_lossy(&buf));
        //let _ = stream.write_all(b"GET / HTTP/1.0\r\n\r\n").await;
    }

    run(listen("127.0.0.1:8888"))
}
