use serde_derive::{Serialize, Deserialize};
use serde_bytes::ByteBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct Packet {
    #[serde(rename="t")]
    pub transaction_id: String,
    #[serde(flatten)]
    pub message: Message,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "y")]
pub enum Message {
    #[serde(rename = "q")]
    Query {
        #[serde(flatten)]
        query: Query,
    },

    #[serde(rename = "r")]
    Response {
        #[serde(rename = "r")]
        response: Response,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "q", content = "a")]
pub enum Query {
    #[serde(rename = "ping")]
    Ping {
        id: String,
    },
    #[serde(rename = "find_node")]
    FindNode {
        id: ByteBuf,
        target: ByteBuf,
    },
    #[serde(rename = "get_peers")]
    GetPeers {
        id: ByteBuf,
        infohash: String,
    },
    #[serde(rename = "announce_peer")]
    AnnouncePeer {
        id: String,
        implied_port: Option<bool>,
        infohash: String,
        port: u32,
        token: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Response {
    FindNode {
        id: ByteBuf,
        nodes: ByteBuf,
        values: Vec<ByteBuf>,
    },
    GetPeers {
        id: ByteBuf,
        token: Option<ByteBuf>,
        nodes: ByteBuf,
    },
    Ping {
        id: ByteBuf,
    },
}
