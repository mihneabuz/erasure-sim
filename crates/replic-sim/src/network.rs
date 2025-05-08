use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
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

pub struct SimNetworkManager {
    inner: Mutex<SimNetworkManagerInner>,
    stats: SimNetworkStatsCounter,
}

impl SimNetworkManager {
    fn new() -> Self {
        Self {
            inner: Mutex::new(SimNetworkManagerInner {
                id: 0,
                senders: HashMap::new(),
                disabled: HashSet::new(),
            }),
            stats: SimNetworkStatsCounter::new(),
        }
    }

    pub fn stats() -> SimNetworkStats {
        MANAGER.stats.get()
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

pub struct SimNetworkStatsCounter {
    successfull_downloads: AtomicU64,
    failed_downloads: AtomicU64,
    messages_sent: AtomicU64,
    bytes_sent: AtomicU64,
}

pub struct SimNetworkStats {
    pub successfull_downloads: u64,
    pub failed_downloads: u64,
    pub messages_sent: u64,
    pub bytes_sent: u64,
}

impl SimNetworkStatsCounter {
    fn new() -> Self {
        Self {
            successfull_downloads: AtomicU64::new(0),
            failed_downloads: AtomicU64::new(0),
            messages_sent: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
        }
    }

    fn increment_successfull_downloads(&self) {
        self.successfull_downloads.fetch_add(1, Ordering::Relaxed);
    }

    fn increment_failed_downloads(&self) {
        self.failed_downloads.fetch_add(1, Ordering::Relaxed);
    }

    fn increment_messages_sent(&self) {
        self.messages_sent.fetch_add(1, Ordering::Relaxed);
    }

    fn increment_bytes_sent(&self, val: u64) {
        self.bytes_sent.fetch_add(val, Ordering::Relaxed);
    }

    fn get(&self) -> SimNetworkStats {
        SimNetworkStats {
            successfull_downloads: self.successfull_downloads.load(Ordering::Relaxed),
            failed_downloads: self.failed_downloads.load(Ordering::Relaxed),
            messages_sent: self.messages_sent.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
        }
    }
}

pub struct SimNetwork {
    id: usize,
    receiver: Mutex<Receiver<(usize, Command)>>,
    latency: usize,
    throughput: usize,
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
        MANAGER.stats.increment_messages_sent();
        MANAGER.stats.increment_bytes_sent(cmd.size() as u64);
        tokio::spawn(MANAGER.forward(self.id, id, cmd));
    }

    async fn recv(&self) -> Option<(String, Command)> {
        let res = self.receiver.lock().await.recv().await?;

        tokio::time::sleep(std::time::Duration::from_millis(
            (self.latency + res.1.size() / self.throughput) as u64,
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
            MANAGER.stats.increment_successfull_downloads();
        } else {
            error!(from = id, file = name, "download failed");
            MANAGER.stats.increment_failed_downloads();
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
