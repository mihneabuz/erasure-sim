use std::{collections::HashMap, sync::Mutex};

use crate::{
    file::File,
    network::{Command, Network, NetworkExt},
};

pub struct Node<N> {
    files: Mutex<HashMap<String, File>>,
    network: N,
}

impl<N: Network> Node<N> {
    pub fn new(network: N) -> Self {
        Self {
            files: Mutex::new(HashMap::new()),
            network,
        }
    }

    pub fn network(&self) -> &N {
        &self.network
    }

    pub async fn upload(&self, name: String, content: String) {
        let mut file = File::encode(content).unwrap();

        let peers = self.network.discover().await;
        for peer in &peers {
            self.network
                .create(peer.clone(), name.clone(), file.metadata().clone())
                .await;
        }

        for shard in file.shards_mut().present_iter() {
            let peer = peers[shard.index() % peers.len()].clone();
            self.network.replicate(peer, name.clone(), shard).await;
        }

        self.files.lock().unwrap().insert(name, file);
    }

    pub async fn download(&self, name: String) -> Option<String> {
        {
            let mut files = self.files.lock().unwrap();
            let file = files.get_mut(&name)?;
            if file.can_decode() {
                return file.decode();
            }
        }

        for peer in self.network.discover().await {
            self.network.request(peer, name.clone()).await;
        }

        None
    }

    pub async fn run(&self) {
        while let Some((peer, cmd)) = self.network.recv().await {
            match cmd {
                Command::Create { name, meta } => {
                    self.files
                        .lock()
                        .unwrap()
                        .entry(name)
                        .or_insert(File::empty(meta));
                }

                Command::Replicate { name, shard } => {
                    self.files
                        .lock()
                        .unwrap()
                        .entry(name)
                        .and_modify(|file| file.shards_mut().merge(shard));
                }

                Command::Request { name } => {
                    let shards = self
                        .files
                        .lock()
                        .unwrap()
                        .get_mut(&name)
                        .into_iter()
                        .flat_map(|file| file.shards_mut().present_iter())
                        .collect::<Vec<_>>();

                    for shard in shards {
                        self.network
                            .replicate(peer.clone(), name.clone(), shard)
                            .await;
                    }
                }
            }
        }
    }
}
