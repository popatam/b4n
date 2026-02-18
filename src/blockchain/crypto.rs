use super::errors::SignError;
use super::{Hash32Type, PubkeyType, SignatureType, Transaction};
use sha2::{Digest, Sha256};

// функций хеширования, sha256 норм
pub fn calc_hash(bytes: &[u8]) -> Hash32Type {
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
pub(crate) fn calc_merkle_root(transactions: &[Transaction]) -> Hash32Type {
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

pub trait Signer {
    fn sign(&self, data: &[u8]) -> Result<SignatureType, SignError>;
}

pub trait Verifier {
    fn verify(&self, pubkey: &PubkeyType, data: &[u8], signature: &SignatureType) -> bool;
}
