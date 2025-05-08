use crate::file::{Metadata, Shard};

#[derive(Clone, Debug)]
pub enum Command {
    Create { name: String, meta: Metadata },
    Replicate { name: String, shard: Shard },
    Request { name: String },
}

impl Command {
    pub fn size(&self) -> usize {
        match self {
            Self::Create { name, .. } => name.len() + std::mem::size_of::<Metadata>(),
            Self::Replicate { name, shard } => name.len() + shard.size(),
            Self::Request { name } => name.len(),
        }
    }
}

#[allow(async_fn_in_trait)]
pub trait Network {
    async fn discover(&self) -> Vec<String>;
    async fn send(&self, peer: String, command: Command);
    async fn recv(&self) -> Option<(String, Command)>;
}

#[allow(async_fn_in_trait)]
pub trait NetworkExt {
    async fn create(&self, peer: String, name: String, meta: Metadata);
    async fn replicate(&self, peer: String, name: String, shard: Shard);
    async fn request(&self, peer: String, name: String);
}

impl<N: Network> NetworkExt for N {
    async fn create(&self, peer: String, name: String, meta: Metadata) {
        self.send(peer, Command::Create { name, meta }).await
    }

    async fn replicate(&self, peer: String, name: String, shard: Shard) {
        self.send(peer, Command::Replicate { name, shard }).await
    }

    async fn request(&self, peer: String, name: String) {
        self.send(peer, Command::Request { name }).await
    }
}
