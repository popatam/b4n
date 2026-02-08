use core::hash::Hash;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

type Hash32Type = [u8; 32];

// функций хеширования, какой алгоритм по умолчанию?
fn calc_hash(bytes: &[u8]) -> Hash32Type {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

// Блок, содержит id, хеш предыдущего блока и транзакцию???, нужна ли дата создания?
#[derive(Debug, Serialize, Deserialize)]
struct Block {
    id: u64,
    previous_hash: Hash32Type,
    transactions: Vec<Transaction>,
}

impl Block {
    fn new(id: u64, previous_hash: Hash32Type, transactions: Vec<Transaction>) -> Block {
        Block {
            id,
            previous_hash,
            transactions,
        }
    }

    fn hash(&self) -> Hash32Type {
        let bytes = self.to_bytes();
        calc_hash(bytes.as_slice())
    }

    fn to_bytes(&self) -> Vec<u8> {
        // через serde_json временно, нужно разобраться как сделать канонично
        serde_json::to_vec(self).expect("Failed to serialize block")
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
        calc_hash(bytes.as_slice())
    }

    fn to_bytes(&self) -> Vec<u8> {
        // через serde_json временно, нужно разобраться как сделать канонично
        serde_json::to_vec(self).expect("Failed to serialize transaction")
    }
}

#[derive(Debug, Serialize)]
struct BlockChain {
    blocks: Vec<Block>,
}

impl BlockChain {
    fn new() -> Self {
        let base_block = Block {
            id: 0,
            previous_hash: [0u8; 32],
            transactions: vec![Transaction::new(0, 0, 0, 0)],
        }; // базовый блок, исключителен, т.к. не содержит ссылки не предыдущий
        BlockChain {
            blocks: vec![base_block],
        }
    }

    fn add_block(&mut self, transactions: Vec<Transaction>) {
        let last_id = self.blocks.len() - 1;
        let last_block = &self.blocks[last_id];
        let next_id = last_block.id + 1;
        let prev_hash = last_block.hash();

        let new_block = Block::new(next_id, prev_hash, transactions);
        self.blocks.push(new_block);
    }

    fn is_valid(&self) -> bool {
        for block_window in self.blocks.windows(2) {
            let [prev_block, cur_block] = block_window else {
                unreachable!();
            };
            if cur_block.previous_hash != prev_block.hash() || cur_block.id != prev_block.id + 1 {
                return false;
            }
        }
        true
    }


    fn get_block(&self, id: u64) -> Option<&Block> {
        self.blocks.get(id as usize)

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
}
