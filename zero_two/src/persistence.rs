pub struct BlockTree<K: KVWrite + KVGet + KVCamera>;

/// A read view into the block tree that is guaranteed to stay unchanged.
pub struct BlockTreeSnapshot<S: KVGet>;


pub trait KVWrite {
    type WB: WriteBatch;

    fn write(&mut self, wb: Self::WB);
}

pub trait KVGet {
    fn get(&mut self, key: Vec<u8>) -> Vec<u8>;
}

/// A trait for key-value stores that can be 'snapshotted', or produce a 'snapshot'. A snapshot is a read
/// view into the key-value store that is guaranteed to stay unchanged.
pub trait KVCamera {
    type S: KVGet;

    fn snapshot(&self) -> Self::S;
}

pub trait WriteBatch {
    fn set(&mut self, key: Vec<u8>, value: Vec<u8>);
}

