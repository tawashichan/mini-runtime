use futures::io::{AsyncReadExt, AsyncWriteExt};
use futures::stream::StreamExt;
use mini_runtime::tcp_listener;
use mini_runtime::tcp_stream;
use mini_runtime::{run, spawn};

async fn http_get(addr: &str) -> Result<String, std::io::Error> {
    let addr = addr.parse().unwrap();
    let mut conn = tcp_stream::AsyncTcpStream::connect(addr)?;
    let _ = conn.write_all(b"GET / HTTP/1.0\r\n\r\n").await?;

    let mut page = Vec::new();
    loop {
        let mut buf = vec![0; 128];

        match conn.read(&mut buf).await {
            Ok(len) => {
                if len == 0 {
                    break;
                }
                page.extend_from_slice(&buf[..len]);
            }
            Err(_) => break,
        }
    }
    let page = String::from_utf8_lossy(&page).into();
    Ok(page)
}
async fn local() {
    match http_get("127.0.0.1:8080").await {
        Ok(resp) => println!("response: {}", resp),
        Err(_) => {}
    }
}

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
    local().await;
    let _ = stream
        .write_all(b"HTTP/1.0 200 OK\nContent-Length: 8\r\n\r\nhogehoge")
        .await;
}

fn main() {
    run(listen("127.0.0.1:8888"))
}
