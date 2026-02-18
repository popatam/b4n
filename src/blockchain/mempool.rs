use super::Hash32Type;
use super::transaction::Transaction;
use std::collections::{HashSet, VecDeque};

pub struct MemPool {
    /// очередь транзакиций на добавление в блок
    queue: VecDeque<Transaction>,
    /// сет прошедших транзакций
    seen: HashSet<Hash32Type>,
}

impl MemPool {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            seen: HashSet::new(),
        }
    }
}

impl MemPool {
    pub fn push(&mut self, transaction: Transaction) -> bool {
        let transaction_hash = transaction.hash();
        if self.seen.contains(&transaction_hash) {
            return false;
        }

        self.seen.insert(transaction_hash);
        self.queue.push_back(transaction);
        true
    }

    pub fn pop_many(&mut self, count: usize) -> Vec<Transaction> {
        let n = count.min(self.queue.len());
        self.queue.drain(..n).collect()
    }
}
