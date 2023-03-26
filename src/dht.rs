use std::collections::{HashMap, BinaryHeap, VecDeque, BTreeMap};
use std::fmt::Display;
use std::net::SocketAddr;
use std::cmp::Reverse;
use std::cmp::Ordering;

use rand::Rng;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NodeId(pub Vec<u8>);

impl NodeId {
    pub fn new() -> Self {
        let mut rng = rand::thread_rng();
        let random_id: Vec<u8> = (0..20).map(|_| rng.gen()).collect();
        NodeId(random_id)
    }
}

impl PartialOrd for NodeId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NodeId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

#[derive(Clone, Debug)]
enum NodeStatus {
    Good,
    Questionable,
    Bad,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: NodeId,
    pub address: SocketAddr,
    status: NodeStatus
}

impl Node {
    pub fn new(id: NodeId, address: SocketAddr) -> Self {
        Self {
            id,
            address,
            status: NodeStatus::Good,
        }
    }
}

#[derive(Clone, Debug)]
pub struct KBucket {
    k: usize,
    nodes: VecDeque<Node>,
}

impl KBucket {
    pub fn new(k: usize) -> Self {
        KBucket {
            k,
            nodes: VecDeque::with_capacity(k),
        }
    }

    pub fn add_node(&mut self, node: Node) {
        if self.nodes.len() < self.k {
            self.nodes.push_back(node);
        } else {
            // Implement your bucket replacement policy here
        }
    }

    pub fn remove_node(&mut self, node_id: &Vec<u8>) {
        self.nodes.retain(|node| &node.id.0 != node_id);
    }

    pub fn find_closest_nodes(&self, target_id: &Vec<u8>, n: usize) -> Vec<Node> {
        let mut sorted_nodes = self.nodes.clone().into_iter().collect::<Vec<_>>();
        sorted_nodes.sort_by_key(|node| xor_distance(&node.id.0, target_id));
        sorted_nodes.into_iter().take(n).collect()
    }
}

#[derive(Debug)]
pub struct DHT {
    local_id: NodeId,
    k: usize,
    b: usize, // number of bits in the node ID
    buckets: BTreeMap<usize, KBucket>,
}

impl DHT {
    pub fn new(local_id: NodeId, k: usize, b: usize) -> Self {
        Self {
            local_id,
            k,
            b,
            buckets: BTreeMap::new(),
        }
    }

    fn bucket_index(&self, id: &Vec<u8>) -> usize {
        // Calculate the bucket index based on the node ID
        (self.b - xor_distance(&self.local_id.0, id).leading_zeros() as usize - 1) % self.b
    }

    pub fn insert(&mut self, node: Node) {
        let idx = self.bucket_index(&node.id.0);
        let bucket = self.buckets.entry(idx).or_insert_with(|| KBucket::new(self.k));
        bucket.add_node(node);
    }

    pub fn remove(&mut self, id: &Vec<u8>) {
        let idx = self.bucket_index(id);
        if let Some(bucket) = self.buckets.get_mut(&idx) {
            bucket.remove_node(id);
            if bucket.nodes.is_empty() {
                self.buckets.remove(&idx);
            }
        }
    }

    pub fn get_nodes(&mut self) -> Vec<Node> {
        let mut nodes = Vec::new();
        for (_, bucket) in self.buckets.iter() {
            nodes.extend(bucket.nodes.iter().cloned());
        }
        nodes
    }

    pub fn get(&self, id: &Vec<u8>) -> Option<&Node> {
        let idx = self.bucket_index(id);
        self.buckets
            .get(&idx)
            .and_then(|bucket| bucket.nodes.iter().find(|node| &node.id.0 == id))
    }

    pub fn get_closest_nodes(&self, target_id: &Vec<u8>, n: usize) -> Vec<Node> {
        let mut closest_nodes = Vec::with_capacity(n);
        let idx = self.bucket_index(target_id);

        for (_, bucket) in self.buckets.range(idx..).rev().chain(self.buckets.range(..idx).rev()) {
            let bucket_nodes = bucket.find_closest_nodes(target_id, n);
            closest_nodes.extend(bucket_nodes);

            if closest_nodes.len() >= n {
                break;
            }
        }

        closest_nodes.sort_by_key(|node| xor_distance(&node.id.0, target_id));
        closest_nodes.into_iter().take(n).collect()
    }
    
}

impl Display for DHT {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (size, bucket) in self.buckets.iter() {
            writeln!(f, "{:?}", bucket)?;
        }
        Ok(())
    }
}

struct NodeDistance<'a> {
    distance: Reverse<NodeId>,
    node: &'a Node,
}

impl<'a> PartialEq for NodeDistance<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.distance == other.distance
    }
}

impl<'a>  Eq for NodeDistance<'a> {}

impl<'a> PartialOrd for NodeDistance<'a> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<'a> Ord for NodeDistance<'a> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.distance.cmp(&other.distance)
    }
}


fn xor_distance(id1: &Vec<u8>, id2: &Vec<u8>) -> u128 {
    let mut distance: u128 = 0;
    for (byte1, byte2) in id1.iter().zip(id2.iter()) {
        distance = (distance << 8) | ((*byte1 ^ *byte2) as u128);
    }
    distance
}
