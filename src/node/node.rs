use crate::blockchain::consensus::{PoAConsensus, PoAConsensusConfig};
use crate::blockchain::errors::SignError;
use crate::blockchain::{
    Block, BlockChain, Hash32Type, MemPool, PubkeyType, SignatureType, Signer, Transaction, Verifier,
};
use crate::transport::ProtocolMessage;
use ed25519_dalek::{Signer as DalekSigner, SigningKey};

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

const MAX_ORPHANS_TOTAL: usize = 1024;
const MAX_ORPHANS_PER_PREV: usize = 8;
const MAX_SEEN_BLOCKS: usize = 100_000;

pub enum NodeMessage {
    /// локальные транзакции через админку или cli
    LocalTrx(Transaction),

    /// сетевое, peer_id известен из handshake
    Net {
        peer_id: u32,
        msg: ProtocolMessage,
    },

    DebugPrint,
    Stop,

    AddPeer {
        peer_id: u32,
        sender: Sender<ProtocolMessage>,
    },
    RemovePeer {
        peer_id: u32,
    },
}

#[derive(Debug)]
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

        eprintln!("pubkey: {:?}, node_id: {:?}", pubkey, node_id);

        Self {
            pubkey,
            private_key: node_id.map(|_| signing_key),
            node_id,
        }
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

pub struct Node<V: Verifier> {
    /// сетевой id ноды, по идее можно заменить открытым ключом
    net_id: u32,

    identity: NodeIdentity,
    chain: BlockChain,
    mempool: MemPool,
    consensus: PoAConsensus<V>,
    peers: HashMap<u32, Sender<ProtocolMessage>>,

    // сюда ли?
    seen_blocks: HashSet<Hash32Type>,
    seen_blocks_order: VecDeque<Hash32Type>,
    orphans_by_prev: HashMap<Hash32Type, Vec<Block>>,
    orphans_total: usize,

    is_syncing: bool,
    sync_peer: Option<u32>,
    sync_next_height: u64,
    sync_limit: u32,
}

impl<V: Verifier> Node<V> {
    pub fn new(net_id: u32, seed: [u8; 32], chain: BlockChain, consensus: PoAConsensus<V>) -> Self {
        let identity = NodeIdentity::new(seed, &consensus.config);

        let mempool = MemPool::new();

        Self {
            net_id,
            identity,
            chain,
            mempool,
            consensus,
            peers: HashMap::new(),

            seen_blocks: HashSet::new(),
            seen_blocks_order: VecDeque::new(),
            orphans_by_prev: HashMap::new(),
            orphans_total: 0,

            is_syncing: false,
            sync_peer: None,
            sync_next_height: 0,
            sync_limit: 256,
        }
    }

    fn mark_seen_block(&mut self, h: Hash32Type) -> bool {
        if !self.seen_blocks.insert(h) {
            return false;
        }
        self.seen_blocks_order.push_back(h);
        while self.seen_blocks.len() > MAX_SEEN_BLOCKS {
            let Some(old) = self.seen_blocks_order.pop_front() else {
                break;
            };
            self.seen_blocks.remove(&old);
        }

        true
    }

    pub fn add_peer(&mut self, peer_id: u32, sender: Sender<ProtocolMessage>) {
        // разобраться как работать с дублями в контексте Sender
        let is_replaced = self.peers.insert(peer_id, sender).is_some();

        if is_replaced {
            eprintln!("[node {}] add_peer REPLACE {}", self.net_id, peer_id);
        } else {
            eprintln!("[node {}] add_peer {}", self.net_id, peer_id);
        }

        // обмен статусами
        if let Some(peer) = self.peers.get(&peer_id) {
            let _ = peer.send(ProtocolMessage::GetStatus);

            let height = self.chain.get_height();
            let last_block_hash = self.chain.last().hash();
            let _ = peer.send(ProtocolMessage::Status {
                height,
                last_block_hash,
            });
        }
    }

    pub fn remove_peer(&mut self, peer_id: u32) {
        let is_exists = self.peers.remove(&peer_id).is_some();
        if !is_exists {
            eprintln!("[node {}] remove_peer DUP {}, ignored", self.net_id, peer_id);
            return;
        }

        eprintln!(
            "[node {}] remove_peer {} peers_left={}",
            self.net_id,
            peer_id,
            self.peers.len()
        );

        if self.sync_peer == Some(peer_id) {
            self.is_syncing = false;
            self.sync_peer = None;
        }
    }

    fn broadcast_block(&self, block: &Block) {
        for peer in self.peers.values() {
            // тут вероятно что то умнее надо, на сейчас игнор если не получилось отправить
            let _ = peer.send(ProtocolMessage::Block(block.clone()));
        }
    }

    fn on_message(&mut self, message: NodeMessage) {
        match message {
            NodeMessage::LocalTrx(trx) => {
                // прилетела транзакция, положить в пул, разослать дальше
                let inserted = self.mempool.push(trx.clone());
                eprintln!(
                    "[node {}] LocalTrx inserted={} hash={:?} text={:?}",
                    self.net_id,
                    inserted,
                    trx.hash(),
                    "..."
                );
                if inserted {
                    self.gossip(ProtocolMessage::Trx(trx));
                }
            }
            NodeMessage::Net { peer_id, msg } => {
                self.on_net(peer_id, msg);
            }
            NodeMessage::DebugPrint => {
                println!("{:?}", self.chain);
                println!("is_valid={:?}", self.chain.is_valid());
                println!("mempool={:?}", self.mempool);
            }
            NodeMessage::Stop => {}
            NodeMessage::AddPeer { peer_id, sender } => self.add_peer(peer_id, sender),
            NodeMessage::RemovePeer { peer_id } => self.remove_peer(peer_id),
        }
    }

    fn on_net(&mut self, peer_id: u32, msg: ProtocolMessage) {
        match msg {
            ProtocolMessage::Hello { .. } | ProtocolMessage::HelloAck { .. } => {
                // handshake уже обработан транспортом
            }

            ProtocolMessage::Trx(trx) => {
                let inserted = self.mempool.push(trx.clone());
                if inserted {
                    self.gossip(ProtocolMessage::Trx(trx));
                }
            }

            ProtocolMessage::Block(block) => {
                let progressed = self.handle_incoming_block(block);
                if progressed {
                    self.try_connect_orphans();
                }
            }

            ProtocolMessage::GetStatus => {
                if let Some(peer) = self.peers.get(&peer_id) {
                    let height = self.chain.get_height();
                    let last_block_hash = self.chain.last().hash();
                    let _ = peer.send(ProtocolMessage::Status {
                        height,
                        last_block_hash,
                    });
                }
            }

            ProtocolMessage::Status {
                height: peer_height, ..
            } => {
                let own_height = self.chain.get_height();
                if peer_height > own_height && !self.is_syncing {
                    self.start_sync(peer_id, own_height + 1);
                }
            }

            ProtocolMessage::GetBlocks { start, limit } => {
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

                if let Some(peer) = self.peers.get(&peer_id) {
                    let _ = peer.send(ProtocolMessage::Blocks { blocks });
                }
            }

            ProtocolMessage::Blocks { blocks } => {
                let mut progressed = false;
                for b in blocks.iter().cloned() {
                    progressed |= self.handle_incoming_block(b);
                }
                if progressed {
                    self.try_connect_orphans();
                }

                // пагинация синка: если пришло ровно limit, то просим следующую пачку
                if self.is_syncing && self.sync_peer == Some(peer_id) {
                    let got = blocks.len() as u32;
                    if got == self.sync_limit {
                        let next = self.chain.get_height() + 1;
                        self.sync_next_height = next;
                        self.request_blocks(peer_id, next);
                    } else {
                        self.is_syncing = false;
                        self.sync_peer = None;
                    }
                }
            }
        }
    }

    fn handle_incoming_block(&mut self, block: Block) -> bool {
        if !self.mark_seen_block(block.hash()) {
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
                if let Some(pid) = self.any_peer_for_sync() {
                    self.start_sync(pid, expected_index);
                }
                return false;
            }

            if self.consensus.validate_block(prev, &block).is_err() {
                // валидация по консенсусу, если не прошла, значит нас хотят обмануть
                return false;
            }

            // добавление блока, обновление state консенсуса, рассылка дальше
            self.chain.add_block(block);

            let last_block = self.chain.last();
            self.mempool.remove_included(last_block.transactions()); // очистка из mempool, иначе множатся

            let height = self.chain.get_height();
            self.consensus.update_state(Some(height), 0);

            let last_block = self.chain.last().clone();
            self.gossip(ProtocolMessage::Block(last_block));
            return true;
        }

        // блок выше, отстаём, нужна синхронизация
        self.put_orphan(block);
        if let Some(pid) = self.any_peer_for_sync() {
            self.start_sync(pid, expected_index);
        }
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

        // проверка слота
        let elapsed_slot = self.consensus.state.round_started_at.elapsed();
        if elapsed_slot < Duration::from_millis(self.consensus.config.slot_duration_ms()) {
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
        let _ = self.mark_seen_block(candidate.hash());

        // обновление состояния консенсуса
        let cur_height = self.chain.get_height();
        self.consensus.update_state(Some(cur_height), 0);

        // раскидать соседям
        self.broadcast_block(&candidate);
    }

    // основной цикл работы ноды
    pub fn run_loop(&mut self, rx: Receiver<NodeMessage>) {
        let tick_every = Duration::from_millis(100); // пока хардкод, можно вынести в конфиг
        let mut next_tick = Instant::now() + tick_every;

        loop {
            let wait = next_tick.saturating_duration_since(Instant::now());

            // растовый вариант каналов FIXME может можно иначе?
            match rx.recv_timeout(wait) {
                Ok(NodeMessage::Stop) => break,
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

    fn gossip(&self, msg: ProtocolMessage) {
        let kind = match &msg {
            ProtocolMessage::Trx(_) => "Trx",
            ProtocolMessage::Block(_) => "Block",
            _ => "Other",
        };

        eprintln!("[node {}] gossip {} to {} peers", self.net_id, kind, self.peers.len());

        for peer in self.peers.values() {
            let _ = peer.send(msg.clone());
        }
    }

    fn any_peer_for_sync(&self) -> Option<u32> {
        self.peers.keys().copied().next()
    }

    fn request_blocks(&self, peer_id: u32, start: u64) {
        if let Some(peer) = self.peers.get(&peer_id) {
            let _ = peer.send(ProtocolMessage::GetBlocks {
                start,
                limit: self.sync_limit,
            });
        }
    }

    fn start_sync(&mut self, peer_id: u32, from_height: u64) {
        if self.is_syncing && self.sync_peer == Some(peer_id) && self.sync_next_height == from_height {
            return;
        }
        self.is_syncing = true;
        self.sync_peer = Some(peer_id);
        self.sync_next_height = from_height;

        self.request_blocks(peer_id, from_height);
    }

    fn put_orphan(&mut self, block: Block) {
        if self.orphans_total >= MAX_ORPHANS_TOTAL {
            return;
        }

        let vec = self.orphans_by_prev.entry(block.header.previous_hash).or_default();
        if vec.len() >= MAX_ORPHANS_PER_PREV {
            return;
        }

        vec.push(block);
        self.orphans_total += 1;
    }

    fn try_connect_orphans(&mut self) {
        loop {
            let last_hash = self.chain.last().hash();

            // список сирот под текущий last_hash, иначе на выход
            let Some(mut vec) = self.orphans_by_prev.remove(&last_hash) else {
                return;
            };

            let prev = self.chain.last();

            // ищем первый валидный блок
            let Some(pos) = vec.iter().position(|b| self.consensus.validate_block(prev, b).is_ok()) else {
                // ничего не подошло, возвращаем списко обратно и выходим
                self.orphans_by_prev.insert(last_hash, vec);
                return;
            };

            // забираем из списка и присоединяем
            let block = vec.swap_remove(pos);
            self.chain.add_block(block);

            let height = self.chain.get_height();
            self.consensus.update_state(Some(height), 0);

            let last_block = self.chain.last().clone();
            self.gossip(ProtocolMessage::Block(last_block));

            self.orphans_total = self.orphans_total.saturating_sub(1);

            // возвращаем обратно остаток под новый last_hash
            if !vec.is_empty() {
                let new_hash = self.chain.last().hash();
                let entry = self.orphans_by_prev.entry(new_hash).or_default();
                for b in vec {
                    if entry.len() >= MAX_ORPHANS_PER_PREV {
                        break;
                    }
                    entry.push(b);
                }
            }
        }
    }
}

pub fn spawn_node<V: Verifier + Send + 'static>(mut node: Node<V>) -> (Sender<NodeMessage>, thread::JoinHandle<()>) {
    // что то как то фу, может в run?
    let (tx, rx) = channel::<NodeMessage>();
    let handle = thread::spawn(move || {
        node.run_loop(rx);
    });
    (tx, handle)
}
