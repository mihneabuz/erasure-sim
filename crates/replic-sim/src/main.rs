use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
    sync::Arc,
};

use erasure_node::{
    network::{Command, Network},
    node::Node,
};
use lazy_static::lazy_static;
use tokio::sync::{
    Mutex,
    mpsc::{Receiver, Sender, channel},
};
use tracing::info;

lazy_static! {
    static ref MANAGER: SimNetworkManager = SimNetworkManager::new();
}

struct SimNetworkManager {
    inner: Mutex<SimNetworkManagerInner>,
}

impl SimNetworkManager {
    fn new() -> Self {
        Self {
            inner: Mutex::new(SimNetworkManagerInner {
                id: 0,
                senders: HashMap::new(),
                disabled: HashSet::new(),
            }),
        }
    }

    async fn spawn(&self) -> SimNode {
        let mut inner = self.inner.lock().await;
        let id = inner.id;
        inner.id += 1;

        let (sender, receiver) = channel(256);
        inner.senders.insert(id, sender);
        let net = SimNetwork {
            id,
            receiver: Mutex::new(receiver),
        };

        info!(id, "spawned node");
        SimNode::new(net)
    }

    async fn disable(&self, id: usize) {
        self.inner.lock().await.disabled.insert(id);
        info!(id, "disabled");
    }

    async fn enable(&self, id: usize) {
        self.inner.lock().await.disabled.remove(&id);
        info!(id, "enabled");
    }

    async fn peers(&self, id: usize) -> Vec<usize> {
        let inner = self.inner.lock().await;
        (0..inner.id)
            .filter(|i| *i != id && !inner.disabled.contains(i))
            .collect()
    }

    async fn forward(&self, from: usize, to: usize, cmd: Command) {
        self.inner
            .lock()
            .await
            .senders
            .get_mut(&to)
            .unwrap()
            .send((from, cmd))
            .await
            .unwrap();
    }
}

struct SimNetworkManagerInner {
    id: usize,
    senders: HashMap<usize, Sender<(usize, Command)>>,
    disabled: HashSet<usize>,
}

struct SimNetwork {
    id: usize,
    receiver: Mutex<Receiver<(usize, Command)>>,
}

impl Network for SimNetwork {
    async fn discover(&self) -> Vec<String> {
        MANAGER
            .peers(self.id)
            .await
            .into_iter()
            .map(|id| format!("{id}"))
            .collect()
    }

    async fn send(&self, peer: String, cmd: Command) {
        info!(from = self.id, to = peer, ?cmd, "sending");
        MANAGER.forward(self.id, peer.parse().unwrap(), cmd).await;
    }

    async fn recv(&self) -> Option<(String, Command)> {
        let res = self.receiver.lock().await.recv().await?;
        info!(from = res.0, to = self.id, cmd =? res.1, "received");
        Some((format!("{}", res.0), res.1))
    }
}

struct SimNode {
    inner: Arc<Node<SimNetwork>>,
}

impl SimNode {
    fn new(network: SimNetwork) -> Self {
        let inner = Arc::new(Node::new(network));
        let inner_clone = Arc::clone(&inner);
        tokio::spawn(async move {
            inner_clone.run().await;
        });
        Self { inner }
    }

    fn id(&self) -> usize {
        self.network().id
    }
}

impl Deref for SimNode {
    type Target = Node<SimNetwork>;

    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

struct Config {
    nodes: usize,
}

fn generate_file() -> (String, String) {
    ("test".to_string(), "content".repeat(1000))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::filter::EnvFilter::from_default_env())
        .init();

    let config = Config { nodes: 32 };

    let mut nodes = Vec::new();
    for _ in 0..config.nodes {
        nodes.push(MANAGER.spawn().await);
    }

    let (name, content) = generate_file();
    nodes[0].upload(name, content).await;

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
}
