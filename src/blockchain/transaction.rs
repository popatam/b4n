use serde::{Deserialize, Serialize};
use super::crypto::calc_hash;
use super::Hash32Type;

// Транзакция, содержится в блоке, содержит id, от кого, кому и дату созадния, по идее ещё полезную ангрузку
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Transaction {
    id: u64,
    from: u64, // не строка ли тут?
    to: u64,
    text: String,
    // created_at: SystemTime,  // пока без времени
}

impl Transaction {
    pub(crate) fn new(id: u64, from: u64, to: u64, text: String) -> Transaction {
        Transaction { id, from, to, text }
    }

    pub(crate) fn hash(&self) -> Hash32Type {
        let bytes = self.to_bytes();
        calc_hash(&bytes)
    }

    fn to_bytes(&self) -> Vec<u8> {
        postcard::to_stdvec(&self).expect("Can't serialize transaction")
    }
}