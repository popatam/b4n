use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;
use sha2::{Digest, Sha256};
use std::collections::{HashSet, VecDeque};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

/*
делаю proof of authority
nonce нужен только если proof of work

*/

const VERSION: u32 = 0; // точно константа?
type Hash32Type = [u8; 32];
type PubkeyType = [u8; 32];
type SignatureType = [u8; 64];
const HEADER_CAPACITY_BYTES: usize = 4 + 8 + 32 + 32 + 8 + 8 + 4 + 64;
const HEADER_WO_SIGN_CAPACITY_BYTES: usize = 4 + 8 + 32 + 32 + 8 + 8 + 4;
const TRX_CAPACITY_BYTES: usize = 32;

#[derive(Debug)]
enum SignError {
    NotValidator,
}

trait Signer {
    fn sign(&self, data: &[u8]) -> Result<SignatureType, SignError>;
}

trait Verifier {
    fn verify(&self, pubkey: &PubkeyType, data: &[u8], signature: &SignatureType) -> bool;
}

/// crypt
use ed25519_dalek::ed25519::SignatureEncoding;
use ed25519_dalek::{Signature, Signer as DalekSigner, SigningKey, Verifier as DalekVerifier, VerifyingKey};

pub struct Ed25519Signer {
    /// приватыный ключ ed25519
    signing_key: SigningKey,
}

impl Ed25519Signer {
    /// создать из сида
    pub fn new_from_seed(seed: [u8; 32]) -> Self {
        Self {
            signing_key: SigningKey::from_bytes(&seed),
        }
    }

    /// получить публичный ключ
    pub fn pubkey(&self) -> PubkeyType {
        self.signing_key.verifying_key().to_bytes()
    }
}

impl Signer for Ed25519Signer {
    fn sign(&self, data: &[u8]) -> Result<SignatureType, SignError> {
        let sig: Signature = self.signing_key.sign(data);
        Ok(sig.to_bytes())
    }
}

/// Проверяет подпись по публичному ключу (Ed25519)
#[derive(Default)]
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

#[derive(Debug, Clone)]
struct Validator {
    pubkey: PubkeyType,
}

#[derive(Debug)]
pub enum ConsensusError {
    InvalidConfig,
    GenesisNotAllowedHere,
}

#[derive(Debug, Clone)]
struct PoAConsensusConfig {
    validators: Vec<Validator>,
    /// сколько времени у proposer на сделать блок
    slot_duration_ms: u64,
    /// период повышения round
    timeout_ms: u64,
    /// max кол-во транзакций на блок, можно уйти от Vec<Transaction> в блоке, но пока оставлю так
    max_trx_per_block: usize,
}

struct PoAConsensusState {
    /// текущий последний индекс блока
    current_height: u64,
    /// текущий раунд (попытка вписать блок)
    current_round: u64,
    /// время старта раунда
    round_started_at: Instant,
}

impl PoAConsensusConfig {
    fn validate_config(&self) -> bool {
        if self.validators.len() < 1 {
            return false;
        }

        let uniq_pubkeys = self
            .validators
            .iter()
            .map(|v| v.pubkey)
            .collect::<HashSet<PubkeyType>>();
        if uniq_pubkeys.len() != self.validators.len() {
            return false;
        }

        true
    }
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
#[derive(Debug, Serialize, Deserialize, Clone)]
struct BlockHeader {
    version: u32,
    index: u64, // он же height
    previous_hash: Hash32Type,
    merkle_root: Hash32Type,
    timestamp: u64,   // как понимаю опционально или нет?
    round: u64,       // номер попытки для текущего индекса (высоты)
    proposer_id: u32, // порядковый номер валидатора в списке валидаторов (см. PoA консенсус)
    #[serde(with = "BigArray")]
    signature: SignatureType, // подпись пропосера, как реализовать (De)Serialize для этого типа?
}

impl BlockHeader {
    fn new_genesis() -> Self {
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
struct Block {
    header: BlockHeader,
    transactions: Vec<Transaction>,
}

impl Block {
    fn new_genesis(genesis_header: BlockHeader) -> Self {
        Self {
            header: genesis_header,
            transactions: Vec::with_capacity(0),
        }
    }
}

#[derive(Debug)]
pub enum BlockError {
    GenesisNotAllowedHere,
    InvalidIndex { expected: u64, got: u64 },
    InvalidPrevHash,
    InvalidMerkleRoot,
    InvalidProposer { expected: u32, got: u32 },
    InvalidSignature,
}

impl Block {
    fn build_unsigned(
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

    fn sign(&mut self, signer: &impl Signer) -> Result<(), SignError> {
        let header_data = self.header_wo_signature_to_bytes();
        self.header.signature = signer.sign(&header_data)?;
        Ok(())
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

        buf[off..off + 8].copy_from_slice(&self.header.round.to_be_bytes());
        off += 8;

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

// Транзакция, содержится в блоке, содержит id, от кого, кому и дату созадния, по идее ещё полезную ангрузку
#[derive(Debug, Serialize, Deserialize, Clone)]
struct Transaction {
    id: u64,
    from: u64, // не строка ли тут?
    to: u64,
    amount: u64,
    // created_at: SystemTime,  // пока без времени
}

impl Transaction {
    fn new(id: u64, from: u64, to: u64, amount: u64) -> Transaction {
        Transaction { id, from, to, amount }
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
    chain_id: u64,
    blocks: Vec<Block>,
}

impl BlockChain {
    fn new(chain_id: u64) -> Self {
        let genesis_header = BlockHeader::new_genesis();
        // базовый блок, исключителен, т.к. не содержит ссылки не предыдущий
        let genesis_block = Block::new_genesis(genesis_header);
        BlockChain {
            chain_id,
            blocks: vec![genesis_block],
        }
    }

    fn add_block(&mut self, block: Block) {
        self.blocks.push(block);
    }

    fn is_valid(&self) -> bool {
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

    fn get_block(&self, index: u64) -> Option<&Block> {
        self.blocks.get(index as usize)
    }

    fn last(&self) -> &Block {
        &self.blocks[self.blocks.len() - 1]
    }

    fn get_height(&self) -> u64 {
        self.last().header.index
    }

    fn get_round(&self) -> u64 {
        self.last().header.round
    }
}

struct PoAConsensus<V: Verifier> {
    config: PoAConsensusConfig,
    state: PoAConsensusState,
    verifier: V,
}

impl<V: Verifier> PoAConsensus<V> {
    fn new(config: PoAConsensusConfig, state: PoAConsensusState, verifier: V) -> Result<Self, ConsensusError> {
        if !config.validate_config() {
            return Err(ConsensusError::InvalidConfig);
        }
        Ok(Self {
            config,
            state,
            verifier,
        })
    }
    pub(crate) fn get_current_round(&self) -> u64 {
        self.state.current_round
    }

    fn update_state(&mut self, height: Option<u64>, round: u64) {
        if let Some(h) = height {
            self.state.current_height = h;
        }
        self.state.current_round = round;
        self.state.round_started_at = Instant::now();
    }

    pub fn expected_proposer(&self, block_index: u64, round: u64) -> (u32, &Validator) {
        debug_assert!(block_index > 0, "expected_proposer must not be called for genesis");

        let proposer_count = self.config.validators.len();
        let proposer_index = (block_index - 1 + round) % proposer_count as u64;
        let validator = &self.config.validators[proposer_index as usize];
        (proposer_index as u32, validator)
    }

    pub fn validate_block(&self, prev: &Block, current: &Block) -> Result<(), BlockError> {
        current.validate(prev)?;

        let (expected_proposer_id, expected_validator) =
            self.expected_proposer(current.header.index, current.header.round);

        if current.header.proposer_id != expected_proposer_id {
            return Err(BlockError::InvalidProposer {
                expected: expected_proposer_id,
                got: current.header.proposer_id,
            });
        }

        // проверка подписи в конце, т.к. дороже
        let bytes = current.header_wo_signature_to_bytes();
        let ok = self
            .verifier
            .verify(&expected_validator.pubkey, &bytes, &current.header.signature);
        if !ok {
            return Err(BlockError::InvalidSignature);
        }

        Ok(())
    }
}

struct MemPool {
    /// очередь транзакиций на добавление в блок
    queue: VecDeque<Transaction>,
    /// сет прошедших транзакций
    seen: HashSet<Hash32Type>,
}

impl MemPool {
    fn push(&mut self, transaction: Transaction) -> bool {
        let transaction_hash = transaction.hash();
        if self.seen.contains(&transaction_hash) {
            return false;
        }

        self.seen.insert(transaction_hash);
        self.queue.push_back(transaction);
        true
    }

    fn pop_many(&mut self, count: usize) -> Vec<Transaction> {
        let n = count.min(self.queue.len());
        self.queue.drain(..n).collect()
    }
}

struct NodeIdentity {
    /// публичный ключ ноды
    pubkey: PubkeyType,

    /// приватный ключ (если валидатор)
    private_key: Option<SigningKey>,

    /// порядковый номер (если валидатор)
    node_id: Option<u32>,
}

impl NodeIdentity {
    // seed нужен в любом случае, если нет в валидаторах, то будет обычным узлом
    fn new(seed: [u8; 32], consensus_config: &PoAConsensusConfig) -> Self {
        let signing_key = SigningKey::from_bytes(&seed);
        let pubkey = signing_key.verifying_key().to_bytes();

        let node_id = consensus_config
            .validators
            .iter()
            .position(|v| v.pubkey == pubkey)
            .map(|idx| idx as u32);

        Self {
            pubkey,
            private_key: node_id.map(|_| signing_key),
            node_id,
        }
    }

    fn is_validator(&self) -> bool {
        self.private_key.is_some() && self.node_id.is_some()
    }

    fn node_id(&self) -> Option<u32> {
        self.node_id
    }
}

impl Signer for NodeIdentity {
    fn sign(&self, data: &[u8]) -> Result<SignatureType, SignError> {
        let signing_key = self.private_key.as_ref().ok_or(SignError::NotValidator)?;
        Ok(signing_key.sign(data).to_bytes())
    }
}

struct Node<V: Verifier> {
    identity: NodeIdentity,
    chain: BlockChain,
    mempool: MemPool,
    consensus: PoAConsensus<V>,
    peers: Vec<Sender<NetMessage>>,
}

enum NetMessage {
    Trx(Transaction),
    Block(Block),
    DebugPrint,
    Stop,
    AddPeer(Sender<NetMessage>),
}
impl<V: Verifier> Node<V> {
    pub fn new(seed: [u8; 32], chain: BlockChain, consensus: PoAConsensus<V>, peers: Vec<Sender<NetMessage>>) -> Self {
        let identity = NodeIdentity::new(seed, &consensus.config);

        let mempool = MemPool {
            queue: VecDeque::new(),
            seen: HashSet::new(),
        };

        Self {
            identity,
            chain,
            mempool,
            consensus,
            peers,
        }
    }

    pub fn add_peer(&mut self, sender: Sender<NetMessage>) {
        // разобраться как работать с дублями в контексте Sender
        self.peers.push(sender);
    }

    fn broancast_block(&self, block: &Block) {
        for peer in &self.peers {
            // тут вероятно что то умнее надо, на сейчас игнор если не получилось отправить
            let _ = peer.send(NetMessage::Block(block.clone()));
        }
    }

    fn on_message(&mut self, message: NetMessage) {
        match message {
            NetMessage::Trx(trx) => {
                // прилетела транзакция
                self.mempool.push(trx);
            }
            NetMessage::Block(block) => {
                // прилетел блок
                // validate chain
                let expected_index = self.chain.get_height() + 1;
                if block.header.index != expected_index {
                    // по взрослому тут должна быть какая то логика
                    return;
                }

                // validate consensus
                let prev = self.chain.last();
                if self.consensus.validate_block(prev, &block).is_err() {
                    return;
                }

                // append to chain
                self.chain.add_block(block);

                // update consensus state
                let height = self.chain.get_height();
                self.consensus.update_state(Some(height), 0);
            }
            NetMessage::DebugPrint => {
                println!("{:?}", self.chain)
            }
            NetMessage::Stop => {} // graceful shutdown
            NetMessage::AddPeer(sender) => { self.add_peer(sender); }
        }
    }

    fn build_block(&mut self, transactions: Vec<Transaction>) -> Result<Block, SignError> {
        let proposer_id = self.identity.node_id.ok_or(SignError::NotValidator)?;
        let last_block = self.chain.last();
        let next_block_id = last_block.header.index + 1;
        let prev_hash = last_block.hash();
        let round = self.consensus.get_current_round();

        let mut new_block = Block::build_unsigned(next_block_id, prev_hash, transactions, round, proposer_id);
        let signer = &self.identity;

        new_block.sign(signer)?;
        Ok(new_block)
    }

    // выполняется если нет сообщений
    pub fn on_tick(&mut self) {
        // плохо, путано, потом переделать

        // подготовка консенсуса
        let elapsed = self.consensus.state.round_started_at.elapsed();
        if elapsed >= Duration::from_millis(self.consensus.config.timeout_ms) {
            // плохо
            let next_round = self.consensus.state.current_round.saturating_add(1);
            self.consensus.update_state(None, next_round);
        }

        let next_height = self.chain.get_height() + 1;
        let round = self.consensus.state.current_round;
        let (expected_proposer_id, _) = self.consensus.expected_proposer(next_height, round);

        //
        if self.identity.node_id() != Some(expected_proposer_id) {
            // я не я, очередь не моя
            return;
        }

        // подготовка транзакций
        let txs = self.mempool.pop_many(self.consensus.config.max_trx_per_block);
        if txs.is_empty() {
            // пустой блок без транзакций нельзя на всякий случай
            return;
        }

        // создание блока
        let candidate = match self.build_block(txs) {
            Ok(b) => b,
            Err(_) => return, // не валидатор / нет ключа
        };

        // валидация блока
        let prev = self.chain.last();
        if self.consensus.validate_block(prev, &candidate).is_err() {
            // кандидат не проходит правила консенсуса, не принимаем
            return;
        }

        // непосредственно добавление
        self.chain.blocks.push(candidate.clone());

        // обновление состояния консенсуса
        let cur_height = self.chain.get_height();
        self.consensus.update_state(Some(cur_height), 0);

        // раскидать соседям
        self.broancast_block(&candidate);
    }

    // основной цикл работы ноды
    pub fn run_loop(&mut self, rx: Receiver<NetMessage>) {
        let tick_every = Duration::from_millis(100); // пока хардкод, можно вынести в конфиг
        let mut next_tick = Instant::now() + tick_every;

        loop {
            let wait = next_tick.saturating_duration_since(Instant::now());

            // растовый вариант каналов FIXME может можно иначе?
            match rx.recv_timeout(wait) {
                Ok(NetMessage::Stop) => break,
                Ok(msg) => self.on_message(msg),
                Err(RecvTimeoutError::Timeout) => {
                    // тишина, переход на следующий тик
                    self.on_tick();
                    next_tick += tick_every;

                    let now = Instant::now();
                    if next_tick + tick_every < now {
                        next_tick = now + tick_every;
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    // источник сообщений умер?
                    break;
                }
            }
        }
    }
}

fn spawn_node<V: Verifier + Send + 'static>(mut node: Node<V>) -> (Sender<NetMessage>, thread::JoinHandle<()>) {
    let (tx, rx) = channel::<NetMessage>();
    let handle = thread::spawn(move || {
        node.run_loop(rx);
    });
    (tx, handle)
}

fn main() {
    let seed1 = [1u8; 32];
    let seed2 = [2u8; 32];
    let seed3 = [3u8; 32];

    let signer1 = Ed25519Signer::new_from_seed(seed1);
    let signer2 = Ed25519Signer::new_from_seed(seed2);
    let signer3 = Ed25519Signer::new_from_seed(seed3);

    let validators = vec![
        Validator {
            pubkey: signer1.pubkey(),
        },
        Validator {
            pubkey: signer2.pubkey(),
        },
        Validator {
            pubkey: signer3.pubkey(),
        },
    ];

    let consensus_config = PoAConsensusConfig {
        validators,
        slot_duration_ms: 10_000,
        timeout_ms: 10_000,
        max_trx_per_block: 100,
    };
    assert!(consensus_config.validate_config());

    //
    let chain1 = BlockChain::new(1);
    let chain2 = BlockChain::new(1);
    let chain3 = BlockChain::new(1);

    let verifier1 = Ed25519Verifier;
    let verifier2 = Ed25519Verifier;
    let verifier3 = Ed25519Verifier;

    let state1 = PoAConsensusState {
        current_height: chain1.get_height(),
        current_round: chain1.get_round(),
        round_started_at: Instant::now(),
    };
    let state2 = PoAConsensusState {
        current_height: chain2.get_height(),
        current_round: chain2.get_round(),
        round_started_at: Instant::now(),
    };
    let state3 = PoAConsensusState {
        current_height: chain3.get_height(),
        current_round: chain3.get_round(),
        round_started_at: Instant::now(),
    };

    let consensus1 = PoAConsensus::new(consensus_config.clone(), state1, verifier1).unwrap();
    let consensus2 = PoAConsensus::new(consensus_config.clone(), state2, verifier2).unwrap();
    let consensus3 = PoAConsensus::new(consensus_config.clone(), state3, verifier3).unwrap();

    let node1 = Node::new(seed1, chain1, consensus1, vec![]);
    let node2 = Node::new(seed2, chain2, consensus2, vec![]);
    let node3 = Node::new(seed3, chain3, consensus3, vec![]);

    let (tx1, h1) = spawn_node(node1);
    let (tx2, h2) = spawn_node(node2);
    let (tx3, h3) = spawn_node(node3);

    //
    let _ = tx1.send(NetMessage::AddPeer(tx2.clone()));
    let _ = tx2.send(NetMessage::AddPeer(tx1.clone()));
    let _ = tx3.send(NetMessage::AddPeer(tx2.clone()));


    let _ = tx1.send(NetMessage::Trx(Transaction::new(1, 0, 0, 0)));
    let _ = tx2.send(NetMessage::Trx(Transaction::new(2, 0, 0, 0)));
    let _ = tx3.send(NetMessage::Trx(Transaction::new(3, 0, 0, 0)));

    thread::sleep(Duration::from_secs(2));
    let _ = tx1.send(NetMessage::DebugPrint);
    let _ = tx2.send(NetMessage::DebugPrint);
    let _ = tx3.send(NetMessage::DebugPrint);

    let _ = tx1.send(NetMessage::Stop);
    let _ = tx2.send(NetMessage::Stop);
    let _ = tx3.send(NetMessage::Stop);

    let _ = h1.join();
    let _ = h2.join();
    let _ = h3.join();
}
