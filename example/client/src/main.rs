use futures::io::{AsyncReadExt, AsyncWriteExt};
use mini_runtime::run;
use mini_runtime::tcp_stream;

use std::io::prelude::*;
use std::net::TcpStream;

fn get_sync() {
    let mut conn = TcpStream::connect("127.0.0.1:8888").unwrap();
    conn.write(b"GET / HTTP/1.0\r\n\r\n");
    let mut page = Vec::new();
    loop {
        let mut buf = vec![0; 128];
        match conn.read(&mut buf) {
            Ok(len) => {
                if len == 0 {
                    break;
                }
                page.extend_from_slice(&buf[..len]);
            }
            Err(_) => break,
        };
    }
    println!("{:?}", page);
}

async fn http_get(addr: &str) -> Result<String, std::io::Error> {
    let addr = addr.parse().unwrap();
    let mut conn = tcp_stream::AsyncTcpStream::connect(addr)?;

    let _ = conn.write_all(b"GET / HTTP/1.0\r\n\r\n").await?;

    let mut page = Vec::new();
    loop {
        let mut buf = vec![0; 128];
        let len = conn.read(&mut buf).await?;
        if len == 0 {
            break;
        }
        page.extend_from_slice(&buf[..len]);
    }
    let page = String::from_utf8_lossy(&page).into();
    Ok(page)
}
async fn local() {
    match http_get("127.0.0.1:8888").await {
        Ok(res) => {
            println!("response: {}", res);
        }
        Err(err) => {
            println!("err: {}", err);
        }
    };
}

fn main() {
    run(local())
}
