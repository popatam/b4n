use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

const VERSION: u32 = 0; // точно константа?
type Hash32Type = [u8; 32];
const HEADER_CAPACITY_BYTES: usize = 4 + 8 + 32 + 32 + 8 + 8;
const TRX_CAPACITY_BYTES: usize = 32;

// функций хеширования, какой алгоритм по умолчанию?
fn calc_hash(bytes: &[u8]) -> Hash32Type {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

fn hash_pair(left: Hash32Type, right: Hash32Type) -> Hash32Type {
    let mut buf = [0u8; 64];
    buf[..32].copy_from_slice(&left);
    buf[32..].copy_from_slice(&right);
    calc_hash(&buf)
}

// Биток использует похожий алгоритм, сейчас есть умнее. Описание https://ru.wikipedia.org/wiki/Дерево_хешей
fn calc_merkle_root(transactions: &[Transaction]) -> Hash32Type {
    let mut result: Vec<Hash32Type> = transactions.iter().map(|trx| trx.hash()).collect();
    if result.is_empty() {
        return calc_hash(&[]);
    }

    while result.len() > 1 {
        if result.len() % 2 == 1 {
            result.push(*result.last().unwrap());
        }
        let mut tmp_result = Vec::with_capacity(result.len() / 2);

        for pair in result.chunks_exact(2) {
            let left = pair[0];
            let right = pair[1];
            let hashed = hash_pair(left, right);
            tmp_result.push(hashed);
        }
        result = tmp_result;
    }
    *result.first().expect("No merkle root!")
}

////
#[derive(Debug, Serialize, Deserialize)]
struct BlockHeader {
    version: u32,
    index: u64, // он же height
    previous_hash: Hash32Type,
    merkle_root: Hash32Type,
    timestamp: u64, // как понимаю опционально
    nonce: u64,     // на будущее
}

#[derive(Debug, Serialize, Deserialize)]
struct Block {
    header: BlockHeader,
    // id: u64,
    // previous_hash: Hash32Type,
    transactions: Vec<Transaction>,
}

impl Block {
    fn new(
        index: u64,
        previous_hash: Hash32Type,
        nonce: u64,
        transactions: Vec<Transaction>,
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
            nonce, // пока заглушка
        };

        Block {
            header,
            transactions,
        }
    }

    fn hash(&self) -> Hash32Type {
        let bytes = self.to_bytes();
        calc_hash(&bytes)
    }

    fn to_bytes(&self) -> [u8; HEADER_CAPACITY_BYTES] {
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

        buf[off..off + 8].copy_from_slice(&self.header.nonce.to_be_bytes());
        off += 8;

        debug_assert!(off == HEADER_CAPACITY_BYTES);
        buf
    }
}

// Транзакция, содержится в блоке??? содержит id, от кого, кому и дату созадния, по идее ещё и кол-во? Количество чего?
#[derive(Debug, Serialize, Deserialize)]
struct Transaction {
    id: u64,
    from: u64, // не строка ли тут?
    to: u64,
    amount: u64,
    // created_at: SystemTime,  // пока без времени
}

impl Transaction {
    fn new(id: u64, from: u64, to: u64, amount: u64) -> Transaction {
        Transaction {
            id,
            from,
            to,
            amount,
        }
    }

    fn hash(&self) -> Hash32Type {
        let bytes = self.to_bytes();
        calc_hash(&bytes)
    }

    fn to_bytes(&self) -> [u8; TRX_CAPACITY_BYTES] {
        let mut buf = [0u8; TRX_CAPACITY_BYTES];
        let mut off = 0usize;

        buf[off..off + 8].copy_from_slice(&self.id.to_be_bytes());
        off += 8;
        buf[off..off + 8].copy_from_slice(&self.from.to_be_bytes());
        off += 8;
        buf[off..off + 8].copy_from_slice(&self.to.to_be_bytes());
        off += 8;
        buf[off..off + 8].copy_from_slice(&self.amount.to_be_bytes());
        off += 8;

        debug_assert!(off == TRX_CAPACITY_BYTES);
        buf
    }
}

#[derive(Debug, Serialize)]
struct BlockChain {
    blocks: Vec<Block>,
}

impl BlockChain {
    fn new() -> Self {
        let timestamp = UNIX_EPOCH
            .duration_since(UNIX_EPOCH)
            .expect("Back to the future!!!")
            .as_secs(); // плохо

        let genesis_header = BlockHeader {
            version: VERSION,
            index: 0,
            previous_hash: Hash32Type::default(),
            merkle_root: calc_hash(&[]),
            timestamp,
            nonce: 0,
        };
        let genesis_block = Block {
            header: genesis_header,
            transactions: Vec::with_capacity(0),
        }; // базовый блок, исключителен, т.к. не содержит ссылки не предыдущий
        BlockChain {
            blocks: vec![genesis_block],
        }
    }

    fn add_block(&mut self, transactions: Vec<Transaction>) {
        let last_id = self.blocks.len() - 1;
        let last_block = &self.blocks[last_id];
        let next_id = last_block.header.index + 1;
        let prev_hash = last_block.hash();
        let nonce = 0u64; // заглушка

        let new_block = Block::new(next_id, prev_hash, nonce, transactions);
        self.blocks.push(new_block);
    }

    fn is_valid(&self) -> bool {
        for block_window in self.blocks.windows(2) {
            let [prev_block, cur_block] = block_window else {
                unreachable!();
            };
            if cur_block.header.previous_hash != prev_block.hash() {
                return false;
            }
            if cur_block.header.index != prev_block.header.index + 1 {
                return false;
            }
            if cur_block.header.merkle_root != calc_merkle_root(&cur_block.transactions) {
                return false;
            }
        }
        true
    }

    fn get_block(&self, index: u64) -> Option<&Block> {
        self.blocks.get(index as usize)
    }
}

fn main() {
    let mut chain = BlockChain::new();

    let transaction = Transaction::new(0, 0, 0, 0);
    let transaction1 = Transaction::new(0, 0, 0, 0);
    let transaction2 = Transaction::new(0, 0, 0, 0);

    chain.add_block(vec![transaction]);
    chain.add_block(vec![transaction1]);
    chain.add_block(vec![transaction2]);

    println!("{:#?}", chain);
    println!("{:#?}", chain.is_valid());

    //
    let b2 = chain.get_block(2).unwrap();
    println!("{:?}", b2);

    //
    println!("valid before: {}", chain.is_valid());
    chain.blocks[1].transactions[0].amount = 999;
    println!("valid after: {}", chain.is_valid());
    assert_eq!(chain.is_valid(), false);
}
