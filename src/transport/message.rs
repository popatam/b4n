use crate::blockchain::{Block, Hash32Type, Transaction};
use serde::{Deserialize, Serialize};

// то что ходит по сети
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ProtocolMessage {
    GetStatus {
        from: u32,
    },
    Status {
        from: u32,
        height: u64,
        last_block_hash: Hash32Type,
    },

    GetBlocks {
        from: u32,
        start: u64,
        limit: u32,
    },
    Blocks {
        from: u32,
        blocks: Vec<Block>,
    },

    Trx(Transaction),
    Block(Block),
}

