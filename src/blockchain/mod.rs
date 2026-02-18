pub(crate) mod block;
pub mod crypto;
pub mod errors;
pub mod mempool;
pub mod transaction;
pub mod consensus;


pub use block::{Block, BlockChain};
pub use transaction::Transaction;
pub use errors::BlockError;
pub use crypto::{Verifier, Signer};
pub use mempool::MemPool;

const VERSION: u32 = 0;
pub type Hash32Type = [u8; 32];
pub type PubkeyType = [u8; 32];
pub type SignatureType = [u8; 64];