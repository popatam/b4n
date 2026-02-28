use super::crypto::{calc_hash, calc_merkle_root};
use super::errors::SignError;
use super::{BlockError, Hash32Type, SignatureType, Signer, Transaction, VERSION};
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;
use std::time::SystemTime;

const HEADER_CAPACITY_BYTES: usize = 4 + 8 + 32 + 32 + 8 + 8 + 4 + 64;
const HEADER_WO_SIGN_CAPACITY_BYTES: usize = 4 + 8 + 32 + 32 + 8 + 8 + 4;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BlockHeader {
    version: u32,
    pub(crate) index: u64, // он же height
    pub(crate) previous_hash: Hash32Type,
    merkle_root: Hash32Type,
    timestamp: u64,              // как понимаю опционально или нет?
    pub(crate) round: u64,       // номер попытки для текущего индекса (высоты)
    pub(crate) proposer_id: u32, // порядковый номер валидатора в списке валидаторов (см. PoA консенсус)
    #[serde(with = "BigArray")]
    pub(crate) signature: SignatureType, // подпись пропосера, как реализовать (De)Serialize для этого типа?
}

impl BlockHeader {
    pub(crate) fn new_genesis() -> Self {
        Self {
            version: VERSION,
            index: 0,
            previous_hash: Hash32Type::default(),
            merkle_root: calc_hash(&[]),
            timestamp: 0,
            round: 0,
            proposer_id: 0,
            signature: [0; 64],
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Block {
    pub(crate) header: BlockHeader,
    transactions: Vec<Transaction>,
}

impl Block {
    fn new_genesis(genesis_header: BlockHeader) -> Self {
        Self {
            header: genesis_header,
            transactions: Vec::with_capacity(0),
        }
    }

    pub(crate) fn build_unsigned(
        index: u64,
        previous_hash: Hash32Type,
        transactions: Vec<Transaction>,
        round: u64,
        proposer_id: u32,
    ) -> Block {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("SystemTime before UNIX EPOCH!")
            .as_secs();

        let header = BlockHeader {
            version: VERSION,
            index,
            previous_hash,
            merkle_root: calc_merkle_root(&transactions),
            timestamp,
            round,
            proposer_id,
            signature: [0; 64], // временно
        };

        Block { header, transactions }
    }

    pub(crate) fn sign(&mut self, signer: &impl Signer) -> Result<(), SignError> {
        let header_data = self.header_wo_signature_to_bytes();
        self.header.signature = signer.sign(&header_data)?;
        Ok(())
    }

    pub fn hash(&self) -> Hash32Type {
        let bytes = self.header_to_bytes();
        calc_hash(&bytes)
    }

    pub fn transactions(&self) -> &[Transaction] {
        &self.transactions
    }

    fn header_to_bytes(&self) -> [u8; HEADER_CAPACITY_BYTES] {
        let mut buf = [0u8; HEADER_CAPACITY_BYTES];
        let mut off = 0usize;

        buf[off..off + 4].copy_from_slice(&self.header.version.to_be_bytes());
        off += 4;

        buf[off..off + 8].copy_from_slice(&self.header.index.to_be_bytes());
        off += 8;

        buf[off..off + 32].copy_from_slice(&self.header.previous_hash);
        off += 32;

        buf[off..off + 32].copy_from_slice(&self.header.merkle_root);
        off += 32;

        buf[off..off + 8].copy_from_slice(&self.header.timestamp.to_be_bytes());
        off += 8;

        buf[off..off + 8].copy_from_slice(&self.header.round.to_be_bytes());
        off += 8;

        buf[off..off + 4].copy_from_slice(&self.header.proposer_id.to_be_bytes());
        off += 4;

        buf[off..off + 64].copy_from_slice(&self.header.signature);
        off += 64;

        debug_assert!(off == HEADER_CAPACITY_BYTES);
        buf
    }

    pub(crate) fn header_wo_signature_to_bytes(&self) -> [u8; HEADER_WO_SIGN_CAPACITY_BYTES] {
        let mut buf = [0u8; HEADER_WO_SIGN_CAPACITY_BYTES];
        let mut off = 0usize;

        buf[off..off + 4].copy_from_slice(&self.header.version.to_be_bytes());
        off += 4;

        buf[off..off + 8].copy_from_slice(&self.header.index.to_be_bytes());
        off += 8;

        buf[off..off + 32].copy_from_slice(&self.header.previous_hash);
        off += 32;

        buf[off..off + 32].copy_from_slice(&self.header.merkle_root);
        off += 32;

        buf[off..off + 8].copy_from_slice(&self.header.timestamp.to_be_bytes());
        off += 8;

        buf[off..off + 8].copy_from_slice(&self.header.round.to_be_bytes());
        off += 8;

        buf[off..off + 4].copy_from_slice(&self.header.proposer_id.to_be_bytes());
        off += 4;

        debug_assert!(off == HEADER_WO_SIGN_CAPACITY_BYTES);
        buf
    }

    pub fn validate(&self, prev: &Block) -> Result<(), BlockError> {
        if self.header.index == 0 {
            return Err(BlockError::GenesisNotAllowedHere);
        }

        let expected_index = prev.header.index + 1;
        if self.header.index != expected_index {
            return Err(BlockError::InvalidIndex {
                expected: expected_index,
                got: self.header.index,
            });
        }

        if self.header.previous_hash != prev.hash() {
            return Err(BlockError::InvalidPrevHash);
        }

        let expected_merkle = calc_merkle_root(&self.transactions);
        if self.header.merkle_root != expected_merkle {
            return Err(BlockError::InvalidMerkleRoot);
        }

        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct BlockChain {
    chain_id: u64,
    blocks: Vec<Block>,
}

impl BlockChain {
    pub(crate) fn new(chain_id: u64) -> Self {
        let genesis_header = BlockHeader::new_genesis();
        // базовый блок, исключителен, т.к. не содержит ссылки не предыдущий
        let genesis_block = Block::new_genesis(genesis_header);
        BlockChain {
            chain_id,
            blocks: vec![genesis_block],
        }
    }

    pub(crate) fn add_block(&mut self, block: Block) {
        self.blocks.push(block);
    }

    pub fn is_valid(&self) -> bool {
        for block_window in self.blocks.windows(2) {
            let [prev_block, cur_block] = block_window else {
                unreachable!();
            };

            // проверка хэша хедера блока
            if cur_block.header.previous_hash != prev_block.hash() {
                return false;
            }
            // проверка индекса (высоты) блокчейна
            if cur_block.header.index != prev_block.header.index + 1 {
                return false;
            }
            // проверка хэша транзакций входящих в блок
            if cur_block.header.merkle_root != calc_merkle_root(&cur_block.transactions) {
                return false;
            }
        }
        true
    }

    pub fn get_block(&self, index: u64) -> Option<&Block> {
        self.blocks.get(index as usize)
    }

    pub fn last(&self) -> &Block {
        &self.blocks[self.blocks.len() - 1]
    }

    pub(crate) fn get_height(&self) -> u64 {
        self.last().header.index
    }

    pub(crate) fn get_round(&self) -> u64 {
        self.last().header.round
    }
}
