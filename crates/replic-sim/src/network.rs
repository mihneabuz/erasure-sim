use std::{
    collections::{HashMap, HashSet},
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
use tracing::{debug, error, info};

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

    async fn spawn(&self, latency: usize, throughput: usize) -> SimNode {
        let mut inner = self.inner.lock().await;
        let id = inner.id;
        inner.id += 1;

        let (sender, receiver) = channel(256);
        inner.senders.insert(id, sender);
        let net = SimNetwork {
            id,
            receiver: Mutex::new(receiver),
            latency,
            throughput,
        };

        debug!(id, "spawned node");
        SimNode::new(net)
    }

    async fn disable(&self, id: usize) {
        self.inner.lock().await.disabled.insert(id);
        debug!(id, "disabled");
    }

    async fn enable(&self, id: usize) {
        self.inner.lock().await.disabled.remove(&id);
        debug!(id, "enabled");
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

pub struct SimNetwork {
    id: usize,
    receiver: Mutex<Receiver<(usize, Command)>>,
    latency: usize,
    throughput: usize,
}

impl SimNetwork {
    fn ms_to_process(&self, cmd: &Command) -> usize {
        let size = match cmd {
            Command::Create { name, .. } => name.len(),
            Command::Replicate { name, shard } => name.len() + shard.size(),
            Command::Request { name } => name.len(),
        };

        self.latency + size / self.throughput
    }
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
        let id = peer.parse().unwrap();
        debug!(from = self.id, to = id, ?cmd, "sending");
        tokio::spawn(MANAGER.forward(self.id, id, cmd));
    }

    async fn recv(&self) -> Option<(String, Command)> {
        let res = self.receiver.lock().await.recv().await?;

        tokio::time::sleep(std::time::Duration::from_millis(
            self.ms_to_process(&res.1) as u64
        ))
        .await;

        debug!(from = res.0, to = self.id, cmd =? res.1, "received");
        Some((format!("{}", res.0), res.1))
    }
}

pub struct SimNode {
    inner: Arc<Node<SimNetwork>>,
}

impl SimNode {
    pub async fn spawn(latency: usize, throughput: usize) -> Self {
        MANAGER.spawn(latency, throughput).await
    }

    pub async fn disable(&self) {
        MANAGER.disable(self.inner.network().id).await
    }

    pub async fn enable(&self) {
        MANAGER.enable(self.inner.network().id).await
    }

    fn new(network: SimNetwork) -> Self {
        let inner = Arc::new(Node::new(network));
        let inner_clone = Arc::clone(&inner);
        tokio::spawn(async move {
            inner_clone.run().await;
        });
        Self { inner }
    }

    pub async fn upload(&self, name: String, content: String) {
        let id = self.inner.network().id;
        info!(to = id, file = name, "uploading");
        self.inner.upload(name, content).await;
    }

    pub async fn download(&self, name: String) -> Option<String> {
        let id = self.inner.network().id;
        info!(from = id, file = name, "downloading");
        let res = self._download(name.clone()).await;

        if res.is_some() {
            info!(from = id, file = name, "download successfull");
        } else {
            error!(from = id, file = name, "download failed");
        }

        res
    }

    async fn _download(&self, name: String) -> Option<String> {
        if let Some(res) = self.inner.download(name.clone()).await {
            return Some(res);
        }

        for _ in 0..1000 {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            if let Some(res) = self.inner.try_download(&name).await {
                return Some(res);
            }
        }

        None
    }
}
