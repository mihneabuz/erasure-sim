use std::io::Write;

use reed_solomon_erasure::galois_8::ReedSolomon;

const SHARD_SIZE: usize = 64;

type Shard = Option<Vec<u8>>;

pub struct Shards {
    inner: Vec<Option<Vec<u8>>>,
}

impl Shards {
    fn new(inner: Vec<Option<Vec<u8>>>) -> Self {
        Self { inner }
    }

    pub fn insert(&mut self, shard: Vec<u8>, index: usize) {
        self.inner[index] = Some(shard);
    }

    fn delete(&mut self, index: usize) {
        self.inner[index] = None;
    }
}

pub struct Metadata {
    len: usize,
    data_shards: usize,
    parity_shards: usize,
}

pub fn encode<S: AsRef<str>>(message: S) -> (Metadata, Shards) {
    let bytes = message.as_ref().as_bytes();
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

    let r = ReedSolomon::new(data_shards, parity_shards).unwrap();

    let mut shard_refs = shards
        .iter_mut()
        .map(|shard| shard.as_mut().unwrap())
        .collect::<Vec<_>>();

    r.encode(&mut shard_refs).unwrap();

    let meta = Metadata {
        len: bytes.len(),
        data_shards,
        parity_shards,
    };

    (meta, Shards::new(shards))
}

pub fn decode(meta: Metadata, mut shards: Shards) -> String {
    let r = ReedSolomon::new(meta.data_shards, meta.parity_shards).unwrap();

    r.reconstruct(&mut shards.inner).unwrap();

    let mut content = shards
        .inner
        .into_iter()
        .take(meta.data_shards)
        .flatten()
        .flatten()
        .collect::<Vec<_>>();

    content.truncate(meta.len);

    String::from_utf8(content).unwrap()
}

#[test]
fn sanity() {
    let s1 = "hello world!";
    let (meta, mut shards) = encode(s1);
    shards.delete(1);
    let s2 = decode(meta, shards);
    assert_eq!(s1, s2);
}
