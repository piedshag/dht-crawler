use async_recursion::async_recursion;
use futures::FutureExt;
use log::{error, trace, warn};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_bencode::value::Value;
use serde_bencode::{from_bytes, to_bytes};
use tokio::sync::{Mutex, oneshot};
use std::collections::HashMap;
use std::default;
use std::future::Future;
use std::net::{SocketAddr, Ipv4Addr, IpAddr, Ipv6Addr};
use std::pin::Pin;
use std::sync::Arc;
use tokio::net::lookup_host;
use tokio::net::UdpSocket;
use tokio::time::{sleep, Duration};
use std::convert::TryInto;

mod dht;
use dht::{DHT, Node, NodeId};

#[derive(Debug, Serialize, Deserialize, PartialEq, Default)]
pub enum MessageType {
    #[serde(rename = "q")]
    Query,
    #[serde(rename = "r")]
    #[default]
    Response,
    #[serde(rename = "e")]
    Error,
}

pub enum DhtQuery {
    Ping,
    FindNode(NodeId),
    GetPeers(NodeId),
    AnnouncePeer(NodeId, u16, Vec<u8>),
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct DhtMessage {
    #[serde(with = "serde_bytes")]
    #[serde(default)]
    pub t: Vec<u8>, 
    #[serde(default)]
    pub y: MessageType,   
    #[serde(default)]    
    pub q: Option<String>, 
    #[serde(default)]      
    pub a: Option<Args>,     
    #[serde(default)] 
    pub r: Option<ReturnValues>,
    #[serde(default)]
    pub e: Option<Error>,
}

impl DhtMessage {
    pub fn new(_type: MessageType) -> Self {
        let mut rng = rand::thread_rng();
        let t: Vec<u8> = (0..2)
            .map(|_| rng.gen_range(0..=255))
            .collect();

        DhtMessage {
            t: t,
            y: _type,
            q: None,
            a: None,
            r: None,
            e: None,
        }
    }

    // pub fn with_transaction_id(mut self) -> Self {
    //     let mut rng = rand::thread_rng();
    //     let t: Vec<u8> = (0..2)
    //         .map(|_| rng.gen_range(0..=255))
    //         .collect();
    //     self.t = Some(t);
    //     self
    // }

    pub fn query(mut self, q: DhtQuery) -> Self {
        match q {
            DhtQuery::Ping => {
                self.q = Some("ping".to_string());
                self.a = Some({
                    let mut a = Args::default();
                    a.id = "abcdefghij0123456789".as_bytes().to_vec();
                    a
                })
            },
            DhtQuery::FindNode(node) => {
                self.q = Some("find_node".to_string());
                self.a = Some({
                    let mut a = Args::default();
                    a.id = "abcdefghij0123456789".as_bytes().to_vec();
                    a.target = Some(node.0);
                    a
                })
            },
            DhtQuery::GetPeers(_) => self.q = Some("get_peers".to_string()),
            DhtQuery::AnnouncePeer(_, _, _) => self.q = Some("announce_peer".to_string()),
        }
        self
    }

    pub fn build(self) -> DhtMessage {
        DhtMessage {
            t: self.t,
            y: self.y,
            q: self.q,
            a: self.a,
            r: self.r,
            e: self.e,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct Args {
    #[serde(with = "serde_bytes")]
    pub id: Vec<u8>,
    #[serde(with = "serde_bytes")]
    #[serde(default)]
    pub target: Option<Vec<u8>>,
    #[serde(with = "serde_bytes")]
    #[serde(default)]
    pub info_hash: Option<Vec<u8>>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(with = "serde_bytes")]
    #[serde(default)]
    pub token: Option<Vec<u8>>,
    #[serde(with = "serde_bytes")]
    #[serde(default)]
    pub nodes: Option<Vec<u8>>,
    #[serde(with = "serde_bytes")]
    #[serde(default)]
    pub values: Option<Vec<u8>>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ReturnValues {
    #[serde(with = "serde_bytes")]
    pub id: Vec<u8>,
    #[serde(with = "serde_bytes")]
    #[serde(default)]
    pub token: Option<Vec<u8>>,
    #[serde(with = "serde_bytes")]
    #[serde(default)]
    pub nodes: Option<Vec<u8>>,
    #[serde(with = "serde_bytes")]
    #[serde(default)]
    pub values: Option<Vec<u8>>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Error {
    pub code: u64,
    #[serde(with = "serde_bytes")]
    pub message: Vec<u8>,
}

type PendingRequests = Arc<Mutex<HashMap<Vec<u8>, oneshot::Sender<DhtMessage>>>>;

#[derive(Clone)]
struct Crawler {
    dht: Arc<Mutex<DHT>>,
    socket: Arc<UdpSocket>,
    pending_requests: PendingRequests,
    local_id: NodeId,
}

impl Crawler {
    fn new(socket: UdpSocket) -> Self {
        let local_id = NodeId::new();
        Crawler { dht: Arc::new(Mutex::new(DHT::new(local_id.clone(), 20, 160))), socket: Arc::new(socket), pending_requests: Arc::new(Mutex::new(HashMap::new())), local_id }
    }

    async fn send_message(&mut self, message: &DhtMessage, addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
        let encoded_msg = to_bytes(&message).unwrap();
        self.socket.send_to(&encoded_msg, &addr).await?;
        Ok(())
    }

    async fn send_request_and_wait_for_response(&mut self, request: DhtMessage, addr: SocketAddr) -> Result<DhtMessage, Box<dyn std::error::Error>> {
        self.send_message(&request, addr).await?;

        let (response_sender, response_receiver) = oneshot::channel();
        self.pending_requests.lock().await.insert(request.t.clone(), response_sender);
        let response = match tokio::time::timeout(Duration::from_secs(5), response_receiver).await? {
            Ok(response) => response,
            Err(_) => {
                self.pending_requests.lock().await.remove(&request.t.clone());
                return Err("Request timed out".into());
            }
        };
        self.pending_requests.lock().await.remove(&request.t.clone());
    
        Ok(response)
    }

    async fn process_responses(&self) {
        let mut buf = [0u8; 1024];
    
        loop {
            let (size, src) = self.socket.recv_from(&mut buf).await.unwrap();
            let msg = match from_bytes::<DhtMessage>(&buf[..size]) {
                Ok(msg) => {
                    msg
                },
                Err(e) => {
                    error!("{}", e);
                    continue;
                }
            };

            // let t = match msg.t.clone() {
            //     Some(t) => t,
            //     None => {
            //         self.handle_message(msg, src).await;
            //         continue;
            //     }
            // };
    
            if let Some(response_sender) = self.pending_requests.lock().await.remove(&msg.t) {
                let _ = response_sender.send(msg);
            } else {
                self.handle_message(msg, src).await;
            }
        }
    }

    async fn insert_node (&mut self, node: Node) {
        self.dht.lock().await.insert(node);
    }

    async fn handle_message(&self, msg: DhtMessage, origin: SocketAddr) {
        match msg.y {
            MessageType::Response => {
                if let Some(r) = msg.r {
                    trace!("Response: {:?}", r);
                }
            },
            MessageType::Query => {
                if let Some(q) = msg.q {
                    match q.as_str() {
                        "ping" => {
                            let response = DhtMessage::new(MessageType::Response)
                                .query(DhtQuery::Ping)
                                .build();
                            let encoded_msg = to_bytes(&response).unwrap();
                            self.socket.send_to(&encoded_msg, &origin).await.unwrap();
                        },
                        _ => {
                            warn!("Unhandled query: {:?}", q);
                        },
                    }
                }
            }
            _ => {
                warn!("Unexpected message type: {:?}", msg.y);
            },
        }
    }

    async fn feed(&mut self, nodes: Vec<Node>) {
        let mut dht = self.dht.lock().await;
        nodes.iter().for_each(|node| dht.insert(node.clone()));
        drop(dht);

        for node in nodes {
            let ping = DhtMessage::new(MessageType::Query)
                .query(DhtQuery::Ping)
                .build();

            let res = self.send_message(&ping, node.address).await;
        }
    }

    async fn introduce(&mut self, address: SocketAddr) {
        let ping = DhtMessage::new(MessageType::Query)
            .query(DhtQuery::Ping)
            .build();

        let res = self.send_request_and_wait_for_response(ping, address).await;
        if let Ok(msg) = res {
            match msg.a {
                Some(a) => {
                    let node = Node::new(NodeId(a.id), address);
                    self.insert_node(node).await;
                },
                None => {
                    warn!("No node id in response");
                }
            }
            let node = Node::new(self.local_id.clone(), address);
            self.insert_node(node).await;
        }
    }

    #[async_recursion]
    async fn find_nodes(&mut self, nodes: Vec<Node>) {
        for node in nodes {
            let msg = DhtMessage::new(MessageType::Query)
                .query(DhtQuery::FindNode(NodeId(generate_random_node_id().into())))
                .build();

            let res = match self.send_request_and_wait_for_response(msg, node.address).await {
                Ok(res) => res,
                Err(e) => {
                    error!("{}", e);
                    continue;
                }
            };

            if let Some(r) = res.r {
                let nodes = get_nodes(r);
                trace!("Found {:?}", nodes);
                self.find_nodes(nodes).await;
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Trace)
        .init();

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    let bootstrap_nodes = vec![
        "router.utorrent.com:6881",
        "router.bittorrent.com:6881",
        "dht.transmissionbt.com:6881",
    ];

    let mut crawler = Crawler::new(socket);

    let crawler_clone = crawler.clone();
    tokio::spawn(async move {
        crawler_clone.process_responses().await;
    });

    for host in bootstrap_nodes {
        let addrs = lookup_host(host).await?;
        for address in addrs {
            crawler.introduce(address).await;
        }
    }

    let nodes = crawler.dht.lock().await.get_nodes();
    crawler.find_nodes(nodes).await;
    
    loop {
        sleep(Duration::from_secs(1)).await;
    }

    Ok(())
}

fn generate_random_node_id() -> [u8; 20] {
    let mut rng = rand::thread_rng();
    let mut node_id = [0u8; 20];
    rng.fill(&mut node_id);
    node_id
}

fn bytes_to_ipaddr(bytes: &[u8; 4]) -> Option<IpAddr> {
    let addr = Ipv4Addr::from(bytes.clone());
    Some(IpAddr::V4(addr))
}

fn get_nodes(r: ReturnValues) -> Vec<Node> {
    let mut nodes_list = vec![];
    match (r.nodes, r.values) {
        (Some(nodes), None) => {
            nodes.chunks(26).for_each(|node| {
                let node_id = NodeId(node[0..20].to_vec());
                let addr = bytes_to_ipaddr(&<[u8; 4]>::try_from(&node[20..24]).unwrap()).unwrap();
                let node_addr = SocketAddr::new(addr, u16::from_be_bytes(node[24..26].try_into().unwrap()));
                nodes_list.push(Node::new(node_id, node_addr));
            });
        }
        (None, Some(values)) => {
            values.chunks(6).for_each(|value| {
                let addr = bytes_to_ipaddr(&<[u8; 4]>::try_from(&value[0..4]).unwrap()).unwrap();
                let node_addr = SocketAddr::new(addr, u16::from_be_bytes(value[4..6].try_into().unwrap()));
                nodes_list.push(Node::new(NodeId(value[0..20].to_vec()), node_addr));
            });
        }
        _ => (),
    }
    nodes_list
}
