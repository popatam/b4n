use crate::blockchain::consensus::{PoAConsensus, PoAConsensusConfig};
use crate::blockchain::errors::SignError;
use crate::blockchain::{
    Block, BlockChain, Hash32Type, MemPool, PubkeyType, SignatureType, Signer, Transaction, Verifier,
};
use crate::transport::ProtocolMessage;
use ed25519_dalek::{Signer as DalekSigner, SigningKey};

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

pub enum NodeMessage {
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
        sender: Sender<ProtocolMessage>,
    },
}

impl NodeMessage {
    /// преобразование из NodeMessage того что можно отравлять по сети
    pub(crate) fn into_net(&self) -> Option<ProtocolMessage> {
        match self {
            NodeMessage::GetStatus { from } => Some(ProtocolMessage::GetStatus { from: *from }),
            NodeMessage::Status {
                from,
                height,
                last_block_hash,
            } => Some(ProtocolMessage::Status {
                from: *from,
                height: *height,
                last_block_hash: *last_block_hash,
            }),
            NodeMessage::GetBlocks { from, start, limit } => Some(ProtocolMessage::GetBlocks {
                from: *from,
                start: *start,
                limit: *limit,
            }),
            NodeMessage::Blocks { from, blocks } => Some(ProtocolMessage::Blocks {
                from: *from,
                blocks: blocks.clone(),
            }),
            NodeMessage::Trx(t) => Some(ProtocolMessage::Trx(t.clone())),
            NodeMessage::Block(b) => Some(ProtocolMessage::Block(b.clone())),

            // это не ходит
            NodeMessage::AddPeer { .. } => None,
            NodeMessage::DebugPrint => None,
            NodeMessage::Stop => None,
        }
    }

    // и обратно
    pub(crate) fn from_net(msg: &ProtocolMessage) -> Self {
        match msg {
            ProtocolMessage::GetStatus { from } => NodeMessage::GetStatus { from: *from },
            ProtocolMessage::Status {
                from,
                height,
                last_block_hash,
            } => NodeMessage::Status {
                from: *from,
                height: *height,
                last_block_hash: *last_block_hash,
            },
            ProtocolMessage::GetBlocks { from, start, limit } => NodeMessage::GetBlocks { from: *from, start: *start, limit: *limit },
            ProtocolMessage::Blocks { from, blocks } => NodeMessage::Blocks { from: *from, blocks: blocks.to_vec() },
            ProtocolMessage::Trx(t) => NodeMessage::Trx(t.clone()),
            ProtocolMessage::Block(b) => NodeMessage::Block(b.clone()),
        }
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
    orphans_by_prev: HashMap<Hash32Type, Vec<Block>>,
    is_syncing: bool,
    last_sync_from: Option<u64>,
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
            orphans_by_prev: HashMap::new(),
            is_syncing: false,
            last_sync_from: None,
        }
    }

    pub fn add_peer(&mut self, peer_id: u32, sender: Sender<ProtocolMessage>) {
        // разобраться как работать с дублями в контексте Sender
        self.peers.insert(peer_id, sender);
    }

    fn broancast_block(&self, block: &Block) {
        for peer in self.peers.values() {
            // тут вероятно что то умнее надо, на сейчас игнор если не получилось отправить
            let _ = peer.send(ProtocolMessage::Block(block.clone()));
        }
    }

    fn on_message(&mut self, message: NodeMessage) {
        match message {
            NodeMessage::Trx(trx) => {
                // прилетела транзакция, положить в пул, разослать дальше
                let is_inserted = self.mempool.push(trx.clone());
                if is_inserted {
                    self.gossip_data(NodeMessage::Trx(trx));
                }
            }

            NodeMessage::Block(block) => {
                let progressed = self.handle_incoming_block(block);
                if progressed {
                    self.try_connect_orphans();
                }
            }

            NodeMessage::Blocks { from: _from, blocks } => {
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

            NodeMessage::GetBlocks { from, start, limit } => {
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
                    let _ = peer.send(ProtocolMessage::Blocks {
                        from: self.net_id,
                        blocks,
                    });
                }
            }

            NodeMessage::GetStatus { from } => {
                let height = self.chain.get_height();
                let last_block_hash = self.chain.last().hash();

                if let Some(peer) = self.peers.get(&from) {
                    let _ = peer.send(ProtocolMessage::Status {
                        from: self.net_id,
                        height,
                        last_block_hash,
                    });
                }
            }

            NodeMessage::Status {
                height: peer_height, ..
            } => {
                let own_height = self.chain.get_height();
                if peer_height > own_height {
                    self.start_sync(own_height + 1);
                }
            }

            NodeMessage::DebugPrint => {
                println!("{:?}", self.chain)
            }

            NodeMessage::Stop => {} // graceful shutdown

            NodeMessage::AddPeer { peer_id, sender } => {
                self.add_peer(peer_id, sender);

                // подключился новый peer, меняемся статусами
                if let Some(peer) = self.peers.get(&peer_id) {
                    let _ = peer.send(ProtocolMessage::GetStatus { from: self.net_id });

                    let height = self.chain.get_height();
                    let last_block_hash = self.chain.last().hash();
                    let _ = peer.send(ProtocolMessage::Status {
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
            self.gossip_data(NodeMessage::Block(last_block));
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

    fn gossip_data(&self, msg: NodeMessage) {
        let Some(wire) = msg.into_net() else {
            return;
        };
        // пока передача всем, переделать!
        for peer in self.peers.values() {
            let _ = peer.send(wire.clone());
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
            let _ = peer.send(ProtocolMessage::GetBlocks {
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
                    self.gossip_data(NodeMessage::Block(last_block));
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

pub fn spawn_node<V: Verifier + Send + 'static>(mut node: Node<V>) -> (Sender<NodeMessage>, thread::JoinHandle<()>) {
    // что то как то фу, может в run?
    let (tx, rx) = channel::<NodeMessage>();
    let handle = thread::spawn(move || {
        node.run_loop(rx);
    });
    (tx, handle)
}
