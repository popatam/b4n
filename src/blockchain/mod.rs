pub(crate) mod block;
pub mod consensus;
pub mod crypto;
pub mod errors;
pub mod mempool;
pub mod transaction;

pub use block::{Block, BlockChain};
pub use crypto::{Signer, Verifier};
pub use errors::BlockError;
pub use mempool::MemPool;
pub use transaction::Transaction;

const VERSION: u32 = 0;
pub type Hash32Type = [u8; 32];
pub type PubkeyType = [u8; 32];
pub type SignatureType = [u8; 64];
