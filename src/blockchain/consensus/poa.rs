use crate::blockchain::errors::BlockError;
use crate::blockchain::{Block, PubkeyType, SignatureType, Verifier};
use ed25519_dalek::{Signature, Verifier as DalekVerifier, VerifyingKey};
use std::collections::HashSet;
use std::time::Instant;

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
pub struct Validator {
    pub(crate) pubkey: PubkeyType,
}

impl Validator {
    pub(crate) fn new(pubkey: PubkeyType) -> Self {
        Self { pubkey }
    }
}

#[derive(Debug)]
pub enum ConsensusError {
    InvalidConfig,
}

#[derive(Debug, Clone)]
pub struct PoAConsensusConfig {
    pub(crate) validators: Vec<Validator>,
    /// сколько времени у proposer на сделать блок
    slot_duration_ms: u64,
    /// период повышения round
    pub(crate) timeout_ms: u64,
    /// max кол-во транзакций на блок, можно уйти от Vec<Transaction> в блоке, но пока оставлю так
    pub(crate) max_trx_per_block: usize,
}

impl PoAConsensusConfig {
    pub(crate) fn new(
        validators: Vec<Validator>,
        slot_duration_ms: u64,
        timeout_ms: u64,
        max_trx_per_block: usize,
    ) -> Self {
        Self {
            validators,
            slot_duration_ms,
            timeout_ms,
            max_trx_per_block,
        }
    }

    pub(crate) fn validate_config(&self) -> bool {
        if self.validators.is_empty() {
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

    pub(crate) fn slot_duration_ms(&self) -> u64 {
        self.slot_duration_ms
    }
}

pub struct PoAConsensusState {
    /// текущий последний индекс блока
    current_height: u64,
    /// текущий раунд (попытка вписать блок)
    pub(crate) current_round: u64,
    /// время старта раунда
    pub(crate) round_started_at: Instant,
}

impl PoAConsensusState {
    pub fn new(current_height: u64, current_round: u64) -> Self {
        Self {
            current_height,
            current_round,
            round_started_at: Instant::now(),
        }
    }
}

pub struct PoAConsensus<V: Verifier> {
    pub(crate) config: PoAConsensusConfig,
    pub(crate) state: PoAConsensusState,
    verifier: V,
}

impl<V: Verifier> PoAConsensus<V> {
    pub(crate) fn new(
        config: PoAConsensusConfig,
        state: PoAConsensusState,
        verifier: V,
    ) -> Result<Self, ConsensusError> {
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

    pub(crate) fn update_state(&mut self, height: Option<u64>, round: u64) {
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
