use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::dht::NodeId;

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