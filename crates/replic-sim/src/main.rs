mod network;

use std::collections::HashSet;

use network::SimNode;
use rand::{
    Rng,
    distr::{Alphabetic, Alphanumeric, Uniform},
    seq::{IndexedRandom, index},
};
use tracing::info;

struct File {
    name: String,
    content: String,
}

impl File {
    pub fn generate(size: usize) -> Self {
        let name = rand::rng()
            .sample_iter(&Alphabetic)
            .take(16)
            .map(char::from)
            .collect();

        let content = rand::rng()
            .sample_iter(&Alphanumeric)
            .take(size)
            .map(char::from)
            .collect();

        Self { name, content }
    }

    pub fn name(&self) -> String {
        self.name.clone()
    }

    pub fn content(&self) -> String {
        self.content.clone()
    }
}

struct Config {
    nodes: usize,

    file_count: usize,
    file_min_size: usize,
    file_max_size: usize,

    network_min_latency: usize,
    network_max_latency: usize,

    network_min_throughput: usize,
    network_max_throughput: usize,

    rounds: usize,
    timeout: usize,
    downloads: usize,
    disable: usize,
}

impl Config {
    pub async fn spawn_nodes(&self) -> Vec<SimNode> {
        let mut nodes = Vec::with_capacity(self.nodes);

        let latency_distribution =
            Uniform::new(self.network_min_latency, self.network_max_latency).unwrap();

        let throughtput_distribution =
            Uniform::new(self.network_min_throughput, self.network_max_throughput).unwrap();

        for _ in 0..self.nodes {
            let latency = rand::rng().sample(latency_distribution);
            let throuput = rand::rng().sample(throughtput_distribution);
            nodes.push(SimNode::spawn(latency, throuput).await);
        }

        info!(count = nodes.len(), "spawned nodes");

        nodes
    }

    pub fn generate_files(&self) -> Vec<File> {
        let mut files = Vec::with_capacity(self.file_count);

        let distribution = Uniform::new(self.file_min_size, self.file_max_size).unwrap();

        for _ in 0..self.file_count {
            let size = rand::rng().sample(distribution);
            files.push(File::generate(size));
        }

        info!(count = files.len(), "generated files");

        files
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::filter::EnvFilter::from_default_env())
        .init();

    let config = Config {
        nodes: 8,

        file_count: 32,
        file_min_size: 256,
        file_max_size: 1024,

        network_min_latency: 10,
        network_max_latency: 30,

        network_min_throughput: 100,
        network_max_throughput: 10000,

        rounds: 4,
        timeout: 5000,
        downloads: 16,
        disable: 3,
    };

    let nodes = config.spawn_nodes().await;
    let files = config.generate_files();

    for file in &files {
        nodes
            .choose(&mut rand::rng())
            .unwrap()
            .upload(file.name(), file.content())
            .await;
    }

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    for round in 0..config.rounds {
        tokio::time::sleep(std::time::Duration::from_millis(config.timeout as u64)).await;

        let sample = index::sample(&mut rand::rng(), nodes.len(), config.disable)
            .into_iter()
            .collect::<HashSet<_>>();
        info!(round, nodes =? sample, "disabling nodes");

        let (mut enabled, mut disabled) = (Vec::new(), Vec::new());
        for (index, node) in nodes.iter().enumerate() {
            if sample.contains(&index) {
                node.disable().await;
                disabled.push(node);
            } else {
                enabled.push(node);
            }
        }

        info!(round, "starting");

        let mut downloads = Vec::new();
        for _ in 0..config.downloads {
            let file = files.choose(&mut rand::rng()).unwrap();
            let node = enabled.choose(&mut rand::rng()).unwrap();
            downloads.push(node.download(file.name()));
        }
        futures::future::join_all(downloads).await;

        info!(round, "done");

        for node in disabled {
            node.enable().await;
        }
    }

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
}
