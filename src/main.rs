use serde_big_array::BigArray;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

/*
делаю proof of authority
nonce нужен только если proof of work

*/



const VERSION: u32 = 0; // точно константа?
type Hash32Type = [u8; 32];
type PubkeyType = [u8; 32];
type SignatureType = [u8; 64];
const HEADER_CAPACITY_BYTES: usize = 4 + 8 + 32 + 32 + 8 + 4 + 4 + 64;
const HEADER_WO_SIGN_CAPACITY_BYTES: usize = 4 + 8 + 32 + 32 + 8 + 4 + 4;
const TRX_CAPACITY_BYTES: usize = 32;


trait Signer {
    fn sign(&self, data: &[u8]) -> SignatureType;
}

trait Verifier {
    fn verify(&self, pubkey: &PubkeyType, data: &[u8], signature: &SignatureType) -> bool;
}



/// crypt
use ed25519_dalek::{Signature, SigningKey, VerifyingKey, Signer as DalekSigner, Verifier as DalekVerifier};


pub struct Ed25519Signer {
    /// приватыный ключ ed25519
    signing_key: SigningKey,
}

impl Ed25519Signer {
    /// создать из сида
    pub fn new_from_seed(seed: [u8; 32]) -> Self {
        Self { signing_key: SigningKey::from_bytes(&seed) }
    }

    /// получить публичный ключ
    pub fn pubkey(&self) -> PubkeyType {
        self.signing_key.verifying_key().to_bytes()
    }
}

impl Signer for Ed25519Signer {
    fn sign(&self, data: &[u8]) -> SignatureType {
        let sig: Signature = self.signing_key.sign(data);
        sig.to_bytes()
    }
}

/// Проверяет подпись по публичному ключу (Ed25519)
pub struct Ed25519Verifier;

impl Verifier for Ed25519Verifier {

    /// проверяет подпись по публичному ключу ed25519
    fn verify(&self, pubkey: &PubkeyType, data: &[u8], signature: &SignatureType) -> bool {
        let vk = match VerifyingKey::from_bytes(pubkey) {
            Ok(v) => v,
            Err(_) => return false,
        };

        let sig = Signature::from_bytes(signature);
        vk.verify(data, &sig).is_ok()
    }
}


struct Validator {
    id: u32,
    pubkey: PubkeyType,
}

struct ConsensusParams {
    validators: Vec<Validator>,
    /// сколько времени у proposer на сделать блок
    slot_duration_ms: u64,
    /// период повышения round
    timeout_ms: u64,
    /// max кол-во транзакций на блок, можно уйти от Vec<Transaction> в блоке, но пока оставлю так
    max_trx_per_block: u32,
}

// функций хеширования, sha256 норм
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
    timestamp: u64,           // как понимаю опционально или нет?
    round: u32,               // номер попытки для текущего индекса (высоты)
    proposer_id: u32,
    #[serde(with = "BigArray")]
    signature: SignatureType, // подпись пропосера, как реализовать (De)Serialize для этого типа?
}

#[derive(Debug, Serialize, Deserialize)]
struct Block {
    header: BlockHeader,
    transactions: Vec<Transaction>,
}

impl Block {
    fn build_unsigned(
        index: u64,
        previous_hash: Hash32Type,
        transactions: Vec<Transaction>,
        round: u32,
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

        Block {
            header,
            transactions,
        }
    }

    fn sign(&mut self, signer: &impl Signer) {
        let header_data = self.header_wo_signature_to_bytes();
        self.header.signature = signer.sign(&header_data);
    }

    fn hash(&self) -> Hash32Type {
        let bytes = self.header_to_bytes();
        calc_hash(&bytes)
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

        buf[off..off + 4].copy_from_slice(&self.header.round.to_be_bytes());
        off += 4;

        buf[off..off + 4].copy_from_slice(&self.header.proposer_id.to_be_bytes());
        off += 4;

        buf[off..off + 64].copy_from_slice(&self.header.signature);
        off += 64;

        debug_assert!(off == HEADER_CAPACITY_BYTES);
        buf
    }


    fn header_wo_signature_to_bytes(&self) -> [u8; HEADER_WO_SIGN_CAPACITY_BYTES] {
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

        buf[off..off + 4].copy_from_slice(&self.header.round.to_be_bytes());
        off += 4;

        buf[off..off + 4].copy_from_slice(&self.header.proposer_id.to_be_bytes());
        off += 4;

        debug_assert!(off == HEADER_WO_SIGN_CAPACITY_BYTES);
        buf
    }
}

// Транзакция, содержится в блоке, содержит id, от кого, кому и дату созадния, по идее ещё и кол-во? Количество чего?
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
            .as_secs(); // плохо или норм?

        let genesis_header = BlockHeader {
            version: VERSION,
            index: 0,
            previous_hash: Hash32Type::default(),
            merkle_root: calc_hash(&[]),
            timestamp,
            round: 0,
            proposer_id: 0,
            signature: [0; 64],
        };

        let genesis_block = Block {
            header: genesis_header,
            transactions: Vec::with_capacity(0),
        }; // базовый блок, исключителен, т.к. не содержит ссылки не предыдущий
        BlockChain {
            blocks: vec![genesis_block],
        }
    }

    fn add_block(&mut self, proposer_id: u32, transactions: Vec<Transaction>, signer: &impl Signer) { // какой тип у private_key
        let last_id = self.blocks.len() - 1;
        let last_block = &self.blocks[last_id];
        let next_id = last_block.header.index + 1;
        let prev_hash = last_block.hash();
        let round = 0; // как менять round??

        let mut new_block = Block::build_unsigned(next_id, prev_hash, transactions, round, proposer_id);

        new_block.sign(signer);
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
    let proposer_id: u32 = 1;

    let seed = [0u8; 32];
    let signer = Ed25519Signer::new_from_seed(seed);


    chain.add_block(proposer_id,  vec![transaction], &signer);
    chain.add_block(proposer_id, vec![transaction1], &signer);
    chain.add_block(proposer_id, vec![transaction2], &signer);

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
