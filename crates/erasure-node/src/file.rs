pub use std::io::Write;

use reed_solomon_erasure::galois_8::ReedSolomon;

const SHARD_SIZE: usize = 64;

#[derive(Clone, Debug)]
pub struct Shards {
    inner: Vec<Option<Vec<u8>>>,
}

pub struct ShardsIter<'a> {
    inner: &'a Shards,
    index: usize,
}

impl Iterator for ShardsIter<'_> {
    type Item = Shard;

    fn next(&mut self) -> Option<Self::Item> {
        let index = self.index;
        self.index += 1;

        match self.inner.inner.get(index)?.as_ref() {
            None => self.next(),
            Some(data) => Some(Shard {
                data: data.clone(),
                index,
            }),
        }
    }
}

#[derive(Clone)]
pub struct Shard {
    index: usize,
    data: Vec<u8>,
}

impl std::fmt::Debug for Shard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shard").field("index", &self.index).finish()
    }
}

impl Shard {
    pub fn size(&self) -> usize {
        self.data.len()
    }

    pub fn index(&self) -> usize {
        self.index
    }
}

impl Shards {
    pub fn insert(&mut self, shard: Vec<u8>, index: usize) {
        self.inner[index] = Some(shard);
    }

    pub fn delete(&mut self, index: usize) {
        self.inner[index] = None;
    }

    pub fn merge(&mut self, shard: Shard) {
        if self.inner[shard.index].is_none() {
            self.inner[shard.index] = Some(shard.data);
        }
    }

    fn present(&self) -> usize {
        self.inner.iter().filter(|data| data.is_some()).count()
    }

    pub fn present_iter(&self) -> ShardsIter<'_> {
        ShardsIter {
            inner: self,
            index: 0,
        }
    }

    pub fn size(&self) -> usize {
        self.inner
            .iter()
            .map(|data| data.as_ref().map(|bytes| bytes.len()).unwrap_or(0))
            .sum()
    }
}

#[derive(Clone, Debug)]
pub struct Metadata {
    len: usize,
    data_shards: usize,
    parity_shards: usize,
}

#[derive(Clone, Debug)]
pub struct File {
    meta: Metadata,
    shards: Shards,
}

impl File {
    pub fn empty(meta: Metadata) -> Self {
        let shards = Shards {
            inner: vec![None; meta.data_shards + meta.parity_shards],
        };

        Self { meta, shards }
    }

    pub fn encode<S: AsRef<str>>(content: S) -> Option<Self> {
        let bytes = content.as_ref().as_bytes();
        let data_shards = bytes.chunks(SHARD_SIZE).count();
        let parity_shards = data_shards;

        let mut shards = (0..data_shards + parity_shards)
            .map(|_| Some(vec![0; SHARD_SIZE]))
            .collect::<Vec<_>>();

        bytes
            .chunks(SHARD_SIZE)
            .zip(shards.iter_mut())
            .for_each(|(chunk, shard)| {
                shard
                    .as_mut()
                    .unwrap()
                    .as_mut_slice()
                    .write_all(chunk)
                    .unwrap();
            });

        let r = ReedSolomon::new(data_shards, parity_shards).ok()?;

        let mut shard_refs = shards
            .iter_mut()
            .map(|shard| shard.as_mut().unwrap())
            .collect::<Vec<_>>();

        r.encode(&mut shard_refs).ok()?;

        let meta = Metadata {
            len: bytes.len(),
            data_shards,
            parity_shards,
        };

        let shards = Shards { inner: shards };

        Some(Self { meta, shards })
    }

    pub fn decode(&self) -> Option<String> {
        let meta = self.metadata();
        if !self.can_decode() {
            return None;
        }

        let mut data = self.shards().clone();

        let r = ReedSolomon::new(meta.data_shards, meta.parity_shards).ok()?;

        r.reconstruct(&mut data.inner).ok()?;

        let mut content = data
            .inner
            .into_iter()
            .take(meta.data_shards)
            .flatten()
            .flatten()
            .collect::<Vec<_>>();

        content.truncate(meta.len);

        String::from_utf8(content).ok()
    }

    pub fn can_decode(&self) -> bool {
        self.shards().present() >= self.metadata().data_shards
    }

    pub fn metadata(&self) -> &Metadata {
        &self.meta
    }

    pub fn shards(&self) -> &Shards {
        &self.shards
    }

    pub fn shards_mut(&mut self) -> &mut Shards {
        &mut self.shards
    }
}
