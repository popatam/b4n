use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
/*
mpsc почти каналы как в гошке (там mpmc), найти норм разбор сравнение FIXME

// go
ch := make(chan Message)
go func() {
    ch <- msg
}()
msg := <-ch

// rust  https://doc.rust-lang.org/book/ch16-02-message-passing.html
let (tx, rx) = channel::<Message>();
std::thread::spawn(move || {
    tx.send(msg).unwrap();
});
let msg = rx.recv().unwrap()

 */
use std::thread;
use std::time::{Duration, Instant, SystemTime};

/*
делаю proof of authority
nonce нужен только если proof of work

*/

const VERSION: u32 = 0;
type Hash32Type = [u8; 32];
type PubkeyType = [u8; 32];
type SignatureType = [u8; 64];
const HEADER_CAPACITY_BYTES: usize = 4 + 8 + 32 + 32 + 8 + 8 + 4 + 64;
const HEADER_WO_SIGN_CAPACITY_BYTES: usize = 4 + 8 + 32 + 32 + 8 + 8 + 4;

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
    text: String,
    // created_at: SystemTime,  // пока без времени
}

impl Transaction {
    fn new(id: u64, from: u64, to: u64, text: String) -> Transaction {
        Transaction { id, from, to, text }
    }

    fn hash(&self) -> Hash32Type {
        let bytes = self.to_bytes();
        calc_hash(&bytes)
    }

    fn to_bytes(&self) -> Vec<u8> {
        postcard::to_stdvec(&self).expect("Can't serialize transaction")
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
    /// сетевой id ноды, по идее можно заменить открытым ключом
    net_id: u32,

    identity: NodeIdentity,
    chain: BlockChain,
    mempool: MemPool,
    consensus: PoAConsensus<V>,
    peers: HashMap<u32, Sender<NetMessage>>,

    // сюда ли?
    seen_blocks: HashSet<Hash32Type>,
    orphans_by_prev: HashMap<Hash32Type, Vec<Block>>,
    is_syncing: bool,
    last_sync_from: Option<u64>,
}

enum NetMessage {
    //
    GetStatus {
        from: u32,
    },
    Status {
        from: u32,
        height: u64,
        last_block_hash: Hash32Type,
    },
    //
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
    //
    DebugPrint,
    Stop,

    AddPeer {
        peer_id: u32,
        sender: Sender<NetMessage>,
    },
}

///// СЕТЬ

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

// то что ходит по сети
#[derive(Debug, Serialize, Deserialize, Clone)]
enum WireMessage {
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

impl WireMessage {
    /// преобразование из NetMessage того что можно отравлять по сети
    fn from_net(msg: &NetMessage) -> Option<Self> {
        match msg {
            NetMessage::GetStatus { from } => Some(WireMessage::GetStatus { from: *from }),
            NetMessage::Status {
                from,
                height,
                last_block_hash,
            } => Some(WireMessage::Status {
                from: *from,
                height: *height,
                last_block_hash: *last_block_hash,
            }),
            NetMessage::GetBlocks { from, start, limit } => Some(WireMessage::GetBlocks {
                from: *from,
                start: *start,
                limit: *limit,
            }),
            NetMessage::Blocks { from, blocks } => Some(WireMessage::Blocks {
                from: *from,
                blocks: blocks.clone(),
            }),
            NetMessage::Trx(t) => Some(WireMessage::Trx(t.clone())),
            NetMessage::Block(b) => Some(WireMessage::Block(b.clone())),

            // это не ходит
            NetMessage::AddPeer { .. } => None,
            NetMessage::DebugPrint => None,
            NetMessage::Stop => None,
        }
    }

    // и обратно
    fn into_net(self) -> NetMessage {
        match self {
            WireMessage::GetStatus { from } => NetMessage::GetStatus { from },
            WireMessage::Status {
                from,
                height,
                last_block_hash,
            } => NetMessage::Status {
                from,
                height,
                last_block_hash,
            },
            WireMessage::GetBlocks { from, start, limit } => NetMessage::GetBlocks { from, start, limit },
            WireMessage::Blocks { from, blocks } => NetMessage::Blocks { from, blocks },
            WireMessage::Trx(t) => NetMessage::Trx(t),
            WireMessage::Block(b) => NetMessage::Block(b),
        }
    }
}

fn write_frame(stream: &mut TcpStream, payload: &[u8]) -> std::io::Result<()> {
    let len_u32: u32 = payload
        .len()
        .try_into()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "frame too big"))?;

    stream.write_all(&len_u32.to_be_bytes())?;
    stream.write_all(payload)?;
    stream.flush()?;
    Ok(())
}

fn read_frame(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;

    // тупая защита от ООМ
    const MAX_FRAME: usize = 16 * 1024 * 1024; // 16MB
    if len > MAX_FRAME {
        let error_msg = format!("frame bigger then {} bytes", MAX_FRAME);
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, error_msg));
    }

    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    Ok(buf)
}

fn spawn_tcp_listener(bind_addr: &str, node_in_tx: Sender<NetMessage>) -> thread::JoinHandle<()> {
    let addr = bind_addr.to_string();

    thread::spawn(move || {
        let listener = TcpListener::bind(&addr).expect("failed to bind tcp listener");
        for incoming in listener.incoming() {
            match incoming {
                Ok(mut stream) => {
                    let tx = node_in_tx.clone();
                    let _ = stream.set_nodelay(true);
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(10))); // вынести в конфиг
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));

                    thread::spawn(move || {
                        loop {
                            let frame = match read_frame(&mut stream) {
                                Ok(f) => f,
                                Err(_) => break, // сдох коннект или EOF или ещё чего
                            };

                            let wire: WireMessage = match postcard::from_bytes(&frame) {
                                Ok(m) => m,
                                Err(_) => continue, // мусор
                            };

                            let _ = tx.send(wire.into_net());
                        }
                    });
                }
                Err(_) => {
                    // тут вероятно должна быть какая то логика
                    continue;
                }
            }
        }
    })
}

fn connect_peer(peer_addr: &str) -> Sender<NetMessage> {
    let (tx, rx) = channel::<NetMessage>();
    let addr = peer_addr.to_string();

    thread::spawn(move || {
        // reconnect loop вместо backoff, разобраться как тут backoff носят
        let mut stream = loop {
            match TcpStream::connect(&addr) {
                Ok(s) => {
                    let _ = s.set_nodelay(true);
                    let _ = s.set_read_timeout(Some(Duration::from_secs(10))); // вынести в конфиг
                    let _ = s.set_write_timeout(Some(Duration::from_secs(10)));
                    break s;
                }
                Err(_) => {
                    thread::sleep(Duration::from_millis(200));
                    continue;
                }
            }
        };

        while let Ok(msg) = rx.recv() {
            let Some(wire) = WireMessage::from_net(&msg) else {
                // локальные управляющие сообщения не шлём по сети
                continue;
            };

            let payload = match postcard::to_stdvec(&wire) {
                Ok(p) => p,
                Err(_) => continue,
            };

            if write_frame(&mut stream, &payload).is_err() {
                // если коннект умер пытаемся переподключиться и продолжить
                stream = loop {
                    match TcpStream::connect(&addr) {
                        Ok(s) => {
                            let _ = s.set_nodelay(true);
                            let _ = s.set_read_timeout(Some(Duration::from_secs(10))); // вынести в конфиг
                            let _ = s.set_write_timeout(Some(Duration::from_secs(10)));
                            break s;
                        }
                        Err(_) => {
                            thread::sleep(Duration::from_millis(200));
                            continue;
                        }
                    }
                };
            }
        }
    });

    tx
}

/////

impl<V: Verifier> Node<V> {
    pub fn new(net_id: u32, seed: [u8; 32], chain: BlockChain, consensus: PoAConsensus<V>) -> Self {
        let identity = NodeIdentity::new(seed, &consensus.config);

        let mempool = MemPool {
            queue: VecDeque::new(),
            seen: HashSet::new(),
        };

        Self {
            net_id,
            identity,
            chain,
            mempool,
            consensus,
            peers: HashMap::new(),

            seen_blocks: HashSet::new(),
            orphans_by_prev: HashMap::new(),
            is_syncing: false,
            last_sync_from: None,
        }
    }

    pub fn add_peer(&mut self, peer_id: u32, sender: Sender<NetMessage>) {
        // разобраться как работать с дублями в контексте Sender
        self.peers.insert(peer_id, sender);
    }

    fn broancast_block(&self, block: &Block) {
        for peer in self.peers.values() {
            // тут вероятно что то умнее надо, на сейчас игнор если не получилось отправить
            let _ = peer.send(NetMessage::Block(block.clone()));
        }
    }

    fn on_message(&mut self, message: NetMessage) {
        match message {
            NetMessage::Trx(trx) => {
                // прилетела транзакция, положить в пул, разослать дальше
                let is_inserted = self.mempool.push(trx.clone());
                if is_inserted {
                    self.gossip_data(NetMessage::Trx(trx));
                }
            }

            NetMessage::Block(block) => {
                let progressed = self.handle_incoming_block(block);
                if progressed {
                    self.try_connect_orphans();
                }
            }

            NetMessage::Blocks { from: _from, blocks } => {
                let mut progressed = false;
                for b in blocks {
                    progressed |= self.handle_incoming_block(b);
                }

                if progressed {
                    self.try_connect_orphans();
                }
                self.is_syncing = false;
                self.last_sync_from = None;
            }

            NetMessage::GetBlocks { from, start, limit } => {
                let mut blocks = Vec::new();
                let mut desired_height = start;
                let to = start.saturating_add(limit as u64);

                while desired_height < to {
                    if let Some(b) = self.chain.get_block(desired_height) {
                        blocks.push(b.clone());
                        desired_height += 1;
                    } else {
                        break;
                    }
                }

                if let Some(peer) = self.peers.get(&from) {
                    let _ = peer.send(NetMessage::Blocks {
                        from: self.net_id,
                        blocks,
                    });
                }
            }

            NetMessage::GetStatus { from } => {
                let height = self.chain.get_height();
                let last_block_hash = self.chain.last().hash();

                if let Some(peer) = self.peers.get(&from) {
                    let _ = peer.send(NetMessage::Status {
                        from: self.net_id,
                        height,
                        last_block_hash,
                    });
                }
            }

            NetMessage::Status {
                height: peer_height, ..
            } => {
                let own_height = self.chain.get_height();
                if peer_height > own_height {
                    self.start_sync(own_height + 1);
                }
            }

            NetMessage::DebugPrint => {
                println!("{:?}", self.chain)
            }

            NetMessage::Stop => {} // graceful shutdown

            NetMessage::AddPeer { peer_id, sender } => {
                self.add_peer(peer_id, sender);

                // подключился новый peer, меняемся статусами
                if let Some(peer) = self.peers.get(&peer_id) {
                    let _ = peer.send(NetMessage::GetStatus { from: self.net_id });

                    let height = self.chain.get_height();
                    let last_block_hash = self.chain.last().hash();
                    let _ = peer.send(NetMessage::Status {
                        from: self.net_id,
                        height,
                        last_block_hash,
                    });
                }
            }
        }
    }

    fn handle_incoming_block(&mut self, block: Block) -> bool {
        if !self.seen_blocks.insert(block.hash()) {
            // уже видели, пропускаем
            return false;
        }

        let own_height = self.chain.get_height();
        let expected_index = own_height + 1;

        if block.header.index <= own_height {
            // блок старый, пропускаем
            return false;
        }

        // если блок следующий цепляем
        if block.header.index == expected_index {
            let prev = self.chain.last();
            // валидация по хешу, если не прошла, либо chain попердолило, либо форк
            if block.header.previous_hash != prev.hash() {
                self.put_orphan(block);
                self.start_sync(expected_index);
                return false;
            }

            if self.consensus.validate_block(prev, &block).is_err() {
                // валидация по консенсусу, если не прошла, значит нас хотят обмануть
                return false;
            }

            // добавление блока, обновление state консенсуса, рассылка дальше
            self.chain.add_block(block);
            let height = self.chain.get_height();
            self.consensus.update_state(Some(height), 0);

            let last_block = self.chain.last().clone();
            self.gossip_data(NetMessage::Block(last_block));
            return true;
        }

        // блок выше, отстаём, нужна синхронизация
        self.put_orphan(block);
        self.start_sync(expected_index);
        false
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

        if self.is_syncing {
            // идёт синхронизация, сидим курим
            return;
        }

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
        self.chain.add_block(candidate.clone());

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

    fn gossip_data(&self, msg: NetMessage) {
        // пока передача всем, переделать!
        for peer in self.peers.values() {
            let _ = peer.send(match &msg {
                NetMessage::Trx(t) => NetMessage::Trx(t.clone()),
                NetMessage::Block(b) => NetMessage::Block(b.clone()),
                _ => continue,
            });
        }
    }

    fn start_sync(&mut self, from_height: u64) {
        if self.is_syncing && self.last_sync_from == Some(from_height) {
            return;
        }
        self.is_syncing = true;
        self.last_sync_from = Some(from_height);

        // запрос недостающих блоков
        let limit: u32 = 256;
        for peer in self.peers.values() {
            let _ = peer.send(NetMessage::GetBlocks {
                from: self.net_id,
                start: from_height,
                limit,
            });
        }
    }

    fn put_orphan(&mut self, block: Block) {
        self.orphans_by_prev
            .entry(block.header.previous_hash)
            .or_default()
            .push(block);
    }

    // попытаться подключить бесхозные блоки
    fn try_connect_orphans(&mut self) {
        loop {
            let last_block_hash = self.chain.last().hash();
            let Some(mut vec) = self.orphans_by_prev.remove(&last_block_hash) else {
                break;
            };

            let mut changed = false;
            for block in vec.drain(..) {
                let prev = self.chain.last();
                if self.consensus.validate_block(prev, &block).is_ok() {
                    // FIXME везде одно и то же при добавлении блока, можно объединить
                    self.chain.add_block(block);
                    let height = self.chain.get_height();
                    self.consensus.update_state(Some(height), 0);
                    changed = true;

                    let last_block = self.chain.last().clone();
                    self.gossip_data(NetMessage::Block(last_block));
                    break;
                }
            }

            if !changed {
                // ничего не прицепить
                break;
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

///// CLI

use std::env;

#[derive(Debug)]
struct CliArgs {
    /// id ноды, вычисляет из seed
    net_id: u32,
    /// host:port
    listen: String,
    /// приватный ключ (вернее то из чего он вычисляется)
    seed: [u8; 32],
    /// открытые ключи валидаторов
    validator_pubkeys: Vec<[u8; 32]>,
    /// соседи
    peers: Vec<(u32, String)>, // [(peer_id, "host:port")]
}

fn print_usage_and_exit() -> ! {
    eprintln!(
        "Usage:
  --listen <ip:port>        tcp bind addr, пример: 0.0.0.0:7001
  --seed <hex64>            приватный ключ (32 bytes hex)
  --validator-pubkey <hex64>  публичный ключ валидатора (может быть несколько)
  --peer <id@ip:port>       сосед в формате 2@10.0.0.12:7001 (может быть несколько)

Пример
  Node3:
    --listen 0.0.0.0:7001 --seed <hex64_3> \\
    --validator-pubkey <hex64_1> --validator-pubkey <hex64_2> --validator-seed <hex64_3> \\
    --peer 1@10.0.0.11:7001 --peer 2@10.0.0.12:7001
"
    );
    std::process::exit(2);
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn hex_to_32(s: &str) -> Result<[u8; 32], String> {
    let bytes = s.as_bytes();
    if bytes.len() != 64 {
        return Err(format!("seed must be 64 hex chars, got {}", bytes.len()));
    }

    let mut out = [0u8; 32];
    for i in 0..32 {
        let hi = hex_val(bytes[2 * i]).ok_or_else(|| format!("invalid hex at pos {}", 2 * i))?;
        let lo = hex_val(bytes[2 * i + 1]).ok_or_else(|| format!("invalid hex at pos {}", 2 * i + 1))?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn parse_peer_spec(s: &str) -> Result<(u32, String), String> {
    //  формат "2@10.0.0.12:7001"
    let Some((id_str, addr)) = s.split_once('@') else {
        return Err("peer must be in format <id@ip:port>".to_string());
    };

    let id: u32 = id_str.parse().map_err(|_| format!("invalid peer id '{}'", id_str))?;

    if addr.trim().is_empty() {
        return Err("peer addr is empty".to_string());
    }

    Ok((id, addr.to_string()))
}

fn parse_args() -> CliArgs {
    let mut it = env::args().skip(1);

    let mut net_id: Option<u32> = None;
    let mut listen: Option<String> = None;
    let mut seed: Option<[u8; 32]> = None;
    let mut validator_pubkeys: Vec<[u8; 32]> = Vec::new();
    let mut peers: Vec<(u32, String)> = Vec::new();

    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--listen" => {
                let v = it.next().unwrap_or_else(|| print_usage_and_exit());
                listen = Some(v);
            }
            "--seed" => {
                let v = it.next().unwrap_or_else(|| print_usage_and_exit());
                seed = Some(hex_to_32(&v).unwrap_or_else(|_| print_usage_and_exit()));
            }
            "--validator-pubkey" => {
                let v = it.next().unwrap_or_else(|| print_usage_and_exit());
                let vs = hex_to_32(&v).unwrap_or_else(|_| print_usage_and_exit());
                validator_pubkeys.push(vs);
            }
            "--peer" => {
                let v = it.next().unwrap_or_else(|| print_usage_and_exit());
                let p = parse_peer_spec(&v).unwrap_or_else(|_| print_usage_and_exit());
                peers.push(p);
            }
            "--help" | "-h" => print_usage_and_exit(),
            _ => {
                eprintln!("unknown arg: {arg}");
                print_usage_and_exit();
            }
        }
    }

    let listen = listen.unwrap_or_else(|| print_usage_and_exit());
    let seed = seed.unwrap_or_else(|| print_usage_and_exit());
    let net_id = u32::from_be_bytes(seed[0..4].try_into().unwrap()); // тут безопасно, т.к. seed уже распаковался

    if validator_pubkeys.is_empty() {
        eprintln!("at least one --validator-seed is required");
        print_usage_and_exit();
    }

    // peer_id не должен совпадать с собой
    for (pid, _) in &peers {
        if *pid == net_id {
            eprintln!("peer id must not be equal to own net-id ({net_id})");
            print_usage_and_exit();
        }
    }

    CliArgs {
        net_id,
        listen,
        seed,
        validator_pubkeys,
        peers,
    }
}

///// админко

use std::io::{BufRead, BufReader};

fn spawn_admin_listener(bind_addr: &str, node_tx: Sender<NetMessage>, from_id: u32) -> thread::JoinHandle<()> {
    let addr = bind_addr.to_string();

    thread::spawn(move || {
        let listener = TcpListener::bind(&addr).expect("failed to bind admin listener");

        for incoming in listener.incoming() {
            let stream = match incoming {
                Ok(s) => s,
                Err(_) => continue,
            };

            let tx = node_tx.clone();

            thread::spawn(move || {
                let mut next_trx_id: u64 = 1; // как и зачем нужн id в транзакциях
                let reader = BufReader::new(stream);

                for line in reader.lines() {
                    let line = match line {
                        Ok(s) => s,
                        Err(_) => break,
                    };
                    let cmd = line.trim();

                    if cmd.is_empty() {
                        continue;
                    }

                    if cmd == "print" {
                        let _ = tx.send(NetMessage::DebugPrint);
                        continue;
                    }

                    if cmd == "stop" {
                        let _ = tx.send(NetMessage::Stop);
                        break;
                    }

                    if let Some(rest) = cmd.strip_prefix("trx ") {
                        let text = rest.trim().to_string();
                        if text.is_empty() {
                            continue;
                        }

                        let trx = Transaction::new(next_trx_id, from_id as u64, 0, text);
                        next_trx_id = next_trx_id.saturating_add(1);

                        let _ = tx.send(NetMessage::Trx(trx));
                        continue;
                    }
                }
            });
        }
    })
}

fn pubkey_from_seed(seed: [u8; 32]) -> PubkeyType {
    let sk = SigningKey::from_bytes(&seed);
    sk.verifying_key().to_bytes()
}

fn main() {
    let args = parse_args();

    let validators: Vec<Validator> = args
        .validator_pubkeys
        .iter()
        .map(|&seed| Validator {
            pubkey: pubkey_from_seed(seed),
        })
        .collect();

    let consensus_config = PoAConsensusConfig {
        validators,
        slot_duration_ms: 10_000,
        timeout_ms: 10_000,
        max_trx_per_block: 100,
    };
    if !consensus_config.validate_config() {
        eprintln!("invalid consensus config");
        std::process::exit(254);
    }

    // chain и консенсус
    let chain = BlockChain::new(1);
    let verifier = Ed25519Verifier;

    let state = PoAConsensusState {
        current_height: chain.get_height(),
        current_round: chain.get_round(),
        round_started_at: Instant::now(),
    };

    let consensus = PoAConsensus::new(consensus_config.clone(), state, verifier).unwrap();

    // нода
    let node = Node::new(args.net_id, args.seed, chain, consensus);
    let (tx, join_handle) = spawn_node(node);

    let _listener_handle = spawn_tcp_listener(&args.listen, tx.clone());

    // админко
    let admin_addr = "127.0.0.1:18000";
    let _ = spawn_admin_listener(admin_addr, tx.clone(), args.net_id);

    // connect peers
    for (peer_id, peer_addr) in args.peers {
        let _ = tx.send(NetMessage::AddPeer {
            peer_id,
            sender: connect_peer(&peer_addr),
        });
    }

    let tx_clone = tx.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(1));
        let trx_id = 1_000_000u64 + args.net_id as u64;
        let _ = tx_clone.send(NetMessage::Trx(Transaction::new(trx_id, 0, 0, "i'm alive!".to_string())));
    });

    let _ = join_handle.join();
}
