use async_recursion::async_recursion;
use futures::FutureExt;
use log::{error, trace, warn};
use rand::Rng;
use serde::{Deserialize, Serialize};

use serde_bencode::{from_bytes, to_bytes};
use std::collections::HashMap;
use tokio::sync::{oneshot, Mutex, RwLock};

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use std::convert::TryInto;
use std::sync::Arc;
use tokio::net::lookup_host;
use tokio::net::UdpSocket;
use tokio::time::{sleep, Duration};

mod dht;
use dht::{Node, NodeId, DHT};

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
        let t: Vec<u8> = (0..6).map(|_| rng.gen_range(0..=255)).collect();

        DhtMessage {
            t,
            y: _type,
            q: None,
            a: None,
            r: None,
            e: None,
        }
    }

    pub fn with_id(mut self, id: Vec<u8>) -> Self {
        let id_clone = id.clone();
        self.a = self.a.map_or_else(
            || {
                let mut a = Args::default();
                a.id = id_clone;
                Some(a)
            },
            |mut a| {
                a.id = id;
                Some(a)
            },
        );
        self
    }

    pub fn query(mut self, q: DhtQuery) -> Self {
        match q {
            DhtQuery::Ping => {
                self.q = Some("ping".to_string());
            }
            DhtQuery::FindNode(node) => {
                self.q = Some("find_node".to_string());
                let id_clone = node.0.clone();
                self.a = self.a.map_or_else(
                    || {
                        let mut a = Args::default();
                        a.target = Some(id_clone);
                        Some(a)
                    },
                    |mut a| {
                        a.target = Some(node.0);
                        Some(a)
                    },
                );
            }
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
    fn new(socket: Arc<tokio::net::UdpSocket>, pending_requests: PendingRequests) -> Self {
        let local_id = NodeId::new();
        Crawler {
            dht: Arc::new(Mutex::new(DHT::new(local_id.clone(), 20, 160))),
            socket: socket,
            pending_requests,
            local_id,
        }
    }

    async fn insert_node(&mut self, node: Node) {
        self.dht.lock().await.insert(node);
    }

    async fn handle_message(&self, msg: DhtMessage, origin: SocketAddr) {
        trace!("Received message: {:?}", msg);
        match msg.y {
            MessageType::Response => {
                if let Some(r) = msg.r {
                    trace!("Response: {:?}", r);
                }
            }
            MessageType::Query => {
                if let Some(q) = msg.q {
                    match q.as_str() {
                        "ping" => {
                            let response = DhtMessage::new(MessageType::Response)
                                .query(DhtQuery::Ping)
                                .build();
                            let encoded_msg = to_bytes(&response).unwrap();
                            self.socket.send_to(&encoded_msg, &origin).await.unwrap();
                        }
                        _ => {
                            warn!("Unhandled query: {:?}", q);
                        }
                    }
                }
            }
            _ => {
                warn!("Unexpected message type: {:?}", msg.y);
            }
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

            let _res = send_message(self.socket.clone(), &ping, node.address).await;
        }
    }

    async fn introduce(&mut self, address: SocketAddr) {
        let ping = DhtMessage::new(MessageType::Query)
            .with_id(self.local_id.clone().0)
            .query(DhtQuery::Ping)
            .build();

        let res = send_request_and_wait_for_response(self.socket.clone(), self.pending_requests.clone(), &ping, address).await;
        if let Ok(msg) = res {
            match msg.r {
                Some(r) => {
                    let node = Node::new(NodeId(r.id), address);
                    trace!("adding node: {:?}", node);
                    self.insert_node(node).await;
                }
                None => {
                    warn!("No node id in response {:?}", msg);
                }
            }
        } else {
            warn!("Error while sending ping: {:?}", res);
        }
    }
}

fn generate_random_node_id() -> [u8; 20] {
    let mut rng = rand::thread_rng();
    let mut node_id = [0u8; 20];
    rng.fill(&mut node_id);
    node_id
}

fn bytes_to_ipaddr(bytes: &[u8; 4]) -> Option<IpAddr> {
    let addr = Ipv4Addr::from(*bytes);
    Some(IpAddr::V4(addr))
}

fn get_nodes(r: ReturnValues) -> Vec<Node> {
    let mut nodes_list = vec![];
    match (r.nodes, r.values) {
        (Some(nodes), None) => {
            nodes.chunks(26).for_each(|node| {
                let node_id = NodeId(node[0..20].to_vec());
                let addr = bytes_to_ipaddr(&<[u8; 4]>::try_from(&node[20..24]).unwrap()).unwrap();
                let node_addr =
                    SocketAddr::new(addr, u16::from_be_bytes(node[24..26].try_into().unwrap()));
                nodes_list.push(Node::new(node_id, node_addr));
            });
        }
        (None, Some(values)) => {
            values.chunks(6).for_each(|value| {
                let addr = bytes_to_ipaddr(&<[u8; 4]>::try_from(&value[0..4]).unwrap()).unwrap();
                let node_addr =
                    SocketAddr::new(addr, u16::from_be_bytes(value[4..6].try_into().unwrap()));
                nodes_list.push(Node::new(NodeId(value[0..20].to_vec()), node_addr));
            });
        }
        _ => (),
    }
    nodes_list
}

async fn send_message(
    socket: Arc<tokio::net::UdpSocket>,
    message: &DhtMessage,
    addr: SocketAddr,
) -> Result<(), Box<dyn std::error::Error + std::marker::Send + Sync>> {
    let encoded_msg = to_bytes(&message).unwrap();
    socket.send_to(&encoded_msg, &addr).await?;
    Ok(())
}

async fn send_request_and_wait_for_response(
    socket: Arc<tokio::net::UdpSocket>,
    pending_requests: PendingRequests,
    request: &DhtMessage,
    addr: SocketAddr,
) -> Result<DhtMessage, Box<dyn std::error::Error + std::marker::Send + Sync>> {
    send_message(socket, &request, addr).await?;

    let (response_sender, response_receiver) = oneshot::channel();
    let x = pending_requests
        .lock()
        .await
        .insert(request.t.clone(), response_sender);
    debug_assert!(x.is_none());
    let response = match tokio::time::timeout(Duration::from_secs(5), response_receiver).await?
    {
        Ok(response) => response,
        Err(_) => {
            pending_requests
                .lock()
                .await
                .remove(&request.t.clone());
            return Err("Request timed out".into());
        }
    };

    trace!("Received response: {:?}", response);
    
    pending_requests
        .lock()
        .await
        .remove(&request.t.clone());

    Ok(response)
}

#[async_recursion]
async fn find_nodes(id: NodeId, socket: Arc<tokio::net::UdpSocket>, pending_requests: PendingRequests, nodes: Vec<Node>) {
    for node in nodes {
        trace!("Finding nodes for: {:?}", node);

        let socket_clone = socket.clone();
        let pending_requests_clone = pending_requests.clone();
        let id_clone = id.clone();
        tokio::spawn(async move {
            let msg = DhtMessage::new(MessageType::Query)
                .with_id(id_clone.0.clone())
                .query(DhtQuery::FindNode(NodeId(generate_random_node_id().into())))
                .build();

            let res = {
                send_request_and_wait_for_response(socket_clone.clone(), pending_requests_clone.clone(), &msg, node.address)
                    .await
            };
    
            match res {
                Ok(res) => {
                    if let Some(r) = res.r {
                        trace!("Found nodes: {:?}", r.nodes);
                        find_nodes(id_clone, socket_clone, pending_requests_clone, get_nodes(r)).await
                    } else {
                        warn!("No nodes found");
                    }
                }
                Err(e) => {
                    error!("{}", e);
                }
            };

        });
    }
}

async fn process_responses(pending_requests: PendingRequests, socket: Arc<tokio::net::UdpSocket>) {
    let mut buf = [0u8; 1024];

    loop {
        let (size, src) = socket.recv_from(&mut buf).await.unwrap();
        let msg = match from_bytes::<DhtMessage>(&buf[..size]) {
            Ok(msg) => msg,
            Err(e) => {
                error!("{}", e);
                continue;
            }
        };

        let mut pending_requests = pending_requests.lock().await;
        if let Some(response_sender) = pending_requests.remove(&msg.t) {
            debug_assert!(response_sender.is_closed() == false);
            let res = response_sender.send(msg);
            if let Err(e) = res {
                error!("unable to send message to oneshot");
            }
        } else {
            trace!("got query: {:?}", msg);
        }

        drop(pending_requests);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Trace)
        .init();

    let socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
    let pending_requests = Arc::new(Mutex::new(HashMap::new()));
    let bootstrap_nodes = vec![
        "router.utorrent.com:6881",
        "router.bittorrent.com:6881",
        "dht.transmissionbt.com:6881",
    ];

    let crawler = Arc::new(Mutex::new(Crawler::new(socket.clone(), pending_requests.clone())));
    let local_id = crawler.lock().await.local_id.clone();
    let pending_requests_clone = pending_requests.clone();
    let socket_clone = socket.clone();
    tokio::spawn(async move {
        process_responses(pending_requests_clone, socket_clone).await;
    });

    for host in bootstrap_nodes {
        let addrs = lookup_host(host).await?;
        for address in addrs {
            crawler.lock().await.introduce(address).await;
            trace!("Introduced to: {:?}", address);
        }
    }

    let crawler_clone = crawler.clone();
    let pending_requests = pending_requests.clone();
    let socket = socket.clone();
    tokio::spawn(async move {
        let nodes = crawler_clone.lock().await.dht.lock().await.get_nodes();
        trace!("Found nodes: {:?}", nodes);
        find_nodes(local_id, socket, pending_requests, nodes).await;
    });

    loop {
        sleep(Duration::from_secs(1)).await;
    }

    Ok(())
}