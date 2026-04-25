#![forbid(unsafe_code)]

pub mod locker;
pub mod store;

pub use locker::MemoryLocker;
pub use store::MemoryStore;
