use async_recursion::async_recursion;
use clap::{App, Arg};
use futures::FutureExt;
use log::{error, trace, warn};
use rand::Rng;
use serde::{Deserialize, Serialize};

use serde_bencode::{from_bytes, to_bytes};
use std::collections::HashMap;

use tokio::sync::{oneshot, Mutex};

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use std::convert::TryInto;
use std::sync::Arc;
use tokio::net::lookup_host;
use tokio::net::UdpSocket;
use tokio::time::{sleep, Duration};

mod dht;
use dht::{Node, NodeId, DHT};

mod protocol;
use protocol::{DhtMessage, DhtQuery, MessageType, ReturnValues};

type PendingRequests = Arc<Mutex<HashMap<Vec<u8>, oneshot::Sender<DhtMessage>>>>;

#[derive(Clone)]
struct Crawler {
    dht: Arc<Mutex<DHT>>,
    socket: Arc<UdpSocket>,
    pending_requests: PendingRequests,
    local_id: NodeId,
}

const WAIT_TIMEOUT: u64 = 10;
const IN_FLIGHT_LIMIT: usize = 500;
// milliseconds between sending messages, how fast do you want to go
const MESSAGE_DELAY: u64 = 1000;

impl Crawler {
    fn new(socket: Arc<tokio::net::UdpSocket>, pending_requests: PendingRequests) -> Self {
        let local_id = NodeId::new();
        Crawler {
            dht: Arc::new(Mutex::new(DHT::new(local_id.clone(), 20, 160))),
            socket,
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

    async fn introduce(&mut self, address: SocketAddr) {
        let ping = DhtMessage::new(MessageType::Query)
            .with_id(self.local_id.clone().0)
            .query(DhtQuery::Ping)
            .build();

        let res = Self::send_request_and_wait_for_response(
            self.socket.clone(),
            self.pending_requests.clone(),
            &ping,
            address,
        )
        .await;
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
        match Self::send_message(socket, request, addr).await {
            Ok(_) => (),
            Err(e) => {
                return Err(format!("Error while sending message: {}", e).into());
            }
        };
    
        let (response_sender, response_receiver) = oneshot::channel();
        let x = pending_requests
            .lock()
            .await
            .insert(request.t.clone(), response_sender);
        debug_assert!(x.is_none());
        let response = match tokio::time::timeout(Duration::from_secs(WAIT_TIMEOUT), response_receiver).await 
        {
            Ok(Ok(response)) => response,
            Ok(Err(_)) | Err(_) => {
                pending_requests.lock().await.remove(&request.t.clone());
                return Err(format!("Request timed out for {}:{}", addr.ip(), addr.port()).into());
            }
        };
    
        trace!("Received response: {:?}", response);
    
        pending_requests.lock().await.remove(&request.t.clone());
    
        Ok(response)
    }
    
    #[async_recursion]
    async fn find_nodes(
        socket: Arc<UdpSocket>,
        pending_requests: PendingRequests,
        local_id: NodeId,
        nodes: Vec<Node>,
    ) {
        for node in nodes {
            let socket_clone = socket.clone();
            let pending_requests_clone = pending_requests.clone();
            let id_clone = local_id.clone();
            tokio::spawn(async move {
                let msg = DhtMessage::new(MessageType::Query)
                    .with_id(id_clone.0.clone())
                    .query(DhtQuery::FindNode(NodeId(generate_random_node_id().into())))
                    .build();
    
                let res = Self::send_request_and_wait_for_response(
                    socket_clone.clone(),
                    pending_requests_clone.clone(),
                    &msg,
                    node.address,
                )
                .await;
    
                match res {
                    Ok(res) => {
                        if let Some(r) = res.r {
                            trace!("Found nodes: {:?}", r.nodes);
                            Self::find_nodes(socket_clone, pending_requests_clone, id_clone, get_nodes(r))
                                .await
                        } else {
                            warn!("No nodes found");
                        }
                    }
                    Err(e) => {
                        warn!("{}", e);
                    }
                };
            });
            tokio::time::sleep(Duration::from_millis(MESSAGE_DELAY)).await
        }
    }

    async fn process_responses(pending_requests: PendingRequests, socket: Arc<tokio::net::UdpSocket>) {
        let mut buf = [0u8; 1024];
    
        loop {
            let (size, _src) = socket.recv_from(&mut buf).await.unwrap();
            let msg = match from_bytes::<DhtMessage>(&buf[..size]) {
                Ok(msg) => msg,
                Err(e) => {
                    error!("Error decoding message: {}", e);
                    continue;
                }
            };
    
            trace!("waiting for lock");
            let mut pending_requests = pending_requests.lock().await;
            trace!("got lock");
            if let Some(response_sender) = pending_requests.remove(&msg.t) {
                trace!("pending requests length: {:?}", pending_requests.len());
                debug_assert!(!response_sender.is_closed());
                let res = response_sender.send(msg);
                if let Err(_e) = res {
                    error!("unable to send message to oneshot");
                }
            } else {
                trace!("got message: {:?}", msg);
            }
        
            drop(pending_requests);
        }
    }

    async fn run(&mut self, bootstrap_nodes: Vec<SocketAddr>) {
        let pending_requests_clone = self.pending_requests.clone();
        let socket_clone = self.socket.clone();
    
        tokio::spawn(async move {
            Self::process_responses(pending_requests_clone, socket_clone).await;
        });
    
        for node in bootstrap_nodes {
            self.introduce(node).await;
        }
    
        let socket_clone = self.socket.clone();
        let pending_requests_clone = self.pending_requests.clone();
        let local_id_clone = self.local_id.clone();
        Crawler::find_nodes(socket_clone, pending_requests_clone, local_id_clone, self.dht.lock().await.get_nodes()).await;
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

#[tokio::main]
async fn run(bootstrap_nodes: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
    let pending_requests = Arc::new(Mutex::new(HashMap::new()));

    let mut crawler = Crawler::new(
        socket.clone(),
        pending_requests.clone(),
    );

    let mut parsed_nodes = vec![];
    for host in bootstrap_nodes {
        let addrs = lookup_host(host).await?;
        for address in addrs {
            parsed_nodes.push(address);
        }
    }

    tokio::spawn(async move {
        crawler.run(parsed_nodes).await;
    });

    loop {
        sleep(Duration::from_secs(1)).await;
    }
}

fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Trace)
        .init();

    let matches = App::new("DHT-Crawler CLI")
        .version("1.0")
        .about("A CLI to handle node lists")
        .arg(
            Arg::with_name("bootstrap nodes")
                .short('b')
                .long("bootstrap")
                .value_name("BOOTSTRAP")
                .help("List of bootstrap nodes in IP:PORT format, separated by commas")
                .takes_value(true),
        )
        .get_matches();

    let bootstrap_nodes = matches
        .value_of("bootstrap nodes")
        .unwrap_or("router.bittorrent.com:6881,router.utorrent.com:6881");
    let nodes: Vec<String> = bootstrap_nodes
        .split(',')
        .map(|s| s.parse::<String>().unwrap())
        .collect();

    println!("Nodes: {:?}", nodes);

    run(nodes).unwrap();
}
