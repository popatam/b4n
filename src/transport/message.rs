use crate::blockchain::{Block, Hash32Type, PubkeyType, Transaction};
use serde::{Deserialize, Serialize};

// то что ходит по сети
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ProtocolMessage {
    // handshake
    Hello { peer_id: u32, pubkey: PubkeyType },
    HelloAck { peer_id: u32 },

    // sync
    GetStatus,
    Status { height: u64, last_block_hash: Hash32Type },

    GetBlocks { start: u64, limit: u32 },
    Blocks { blocks: Vec<Block> },

    // gossip
    Trx(Transaction),
    Block(Block),
}
