use std::env;
use std::error::Error;
use tokio::net::UdpSocket;
use std::net::SocketAddr;

mod crawler;
mod protocol;

#[tokio::main]
async fn main () -> Result<(), Box<dyn Error>> {
    let local_addr: SocketAddr = "0.0.0.0:0".parse()?;
    let socket = UdpSocket::bind(local_addr).await?;
    let mut c = crawler::Crawler::new(vec![String::from("82.221.103.244:6881")], socket);
    c.start().await?;
    println!("{:?}", c);
    Ok(())
}
