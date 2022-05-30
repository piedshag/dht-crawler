use tokio::net::UdpSocket;
use std::hash::Hash;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::io;
use std::error::Error;
use std::collections::HashMap;
use sha1::{Sha1, Digest};
use std::str;
use serde_bytes::ByteBuf;
use serde_bencode::de;

use crate::protocol::{Packet, Message, Query, Response};

#[derive(Debug)]
pub struct Crawler {
    bootstrap_nodes: Vec<String>,
    id: Vec<u8>,
    socket: UdpSocket,
}

fn generate_sha1_id() -> Vec<u8> {
    let mut hasher = Sha1::new();
    hasher.update(b"-BD030gyfgjh0-");
    return hasher.finalize().to_vec();
}

impl Crawler {
    pub fn new(bootstrap_nodes: Vec<String>, socket: UdpSocket) -> Self {
        return Crawler{ 
            bootstrap_nodes: bootstrap_nodes,
            id: generate_sha1_id(), 
            socket: socket
        }
    }

    pub async fn send_ping (&self, to: &str) -> Result<(), Box<dyn Error>> {
        let packet = Packet {
            transaction_id: String::from("safsdfaa"),
            message: Message::Query {
                query: Query::Ping { id: self.id.clone().into_iter().map(|i| i.to_string()).collect::<String>() }
            },
        };

        let bytes = serde_bencode::to_bytes(&packet)?;
        let address: SocketAddr = to.parse()?;
        self.socket.send_to(&bytes, address).await?;

        Ok(())
    }

    pub async fn send_find_nodes (&self, to: &str, target_id: ByteBuf) -> Result<(), Box<dyn Error>> {
        let packet = Packet {
            transaction_id: String::from("safsdfaasadjfsdafnja"),
            message: Message::Query {
                query: Query::FindNode {
                    id: ByteBuf::from(self.id.clone()),
                    target: target_id,
                }
            },
        };

        let bytes = serde_bencode::to_bytes(&packet)?;
        let address: SocketAddr = to.parse()?;
        self.socket.send_to(&bytes, address).await?;

        Ok(())
    }

    pub async fn send_get_peers (&self, to: &str, info_hash: &str) -> Result<(), Box<dyn Error>> {
        let packet = Packet {
            transaction_id: String::from("safsdfasfdsffsga"),
            message: Message::Query {
                query: Query::GetPeers {
                    id: ByteBuf::from(self.id.clone()),
                    infohash: info_hash.to_string(),
                }
            },
        };

        let bytes = serde_bencode::to_bytes(&packet)?;
        let address: SocketAddr = to.parse()?;
        self.socket.send_to(&bytes, address).await?;

        Ok(())
    }

    pub async fn handle_ping (&self, id: ByteBuf) -> Result<(), Box<dyn Error>> {
        self.send_find_nodes("87.98.162.88:6881", id).await?;
        Ok(())
    }

    pub async fn handle_find_nodes(&self, nodes: ByteBuf, values: Vec<ByteBuf>) -> Result<(), Box<dyn Error>> {
        println!("{:?}", values);
        Ok(())
    }

    pub async fn start(&mut self) -> Result<(), Box<dyn Error>> {
        self.send_ping("87.98.162.88:6881").await?;

        const MAX_DATAGRAM_SIZE: usize = 65_507;

        loop {
            let mut data = [0u8; MAX_DATAGRAM_SIZE];
            let (len, socket) = self.socket.recv_from(&mut data).await?;

            let response = de::from_bytes::<Packet>(&data[..len])?;
            println!("{:?}", response);
            println!("{:?}", len);
            
            match response.message {
                Message::Response {response} => {
                    match response {
                        Response::Ping {id} => self.handle_ping(id).await?,
                        Response::FindNode { id, nodes, values} => self.handle_find_nodes(nodes, values).await?,
                        Response::GetPeers { id, token, nodes } => println!("{:?}", nodes),
                    }
                }
                _ => println!("Unsupported response")
            }
        }

        Ok(())
    }
}
