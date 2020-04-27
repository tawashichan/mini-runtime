use futures::io::{AsyncReadExt, AsyncWriteExt};
use mini_runtime::run;
use mini_runtime::tcp_stream;

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
    let res = http_get("127.0.0.1:8888").await.unwrap();
    println!("response: {}", res);
}

fn main() {
    run(local())
}
