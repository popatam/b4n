use super::Hash32Type;
use super::transaction::Transaction;
use std::collections::{HashSet, VecDeque};

const MAX_MEMPOOL: usize = 10_000;
const MAX_SEEN: usize = 50_000;

#[derive(Debug)]
pub struct MemPool {
    /// очередь транзакиций на добавление в блок
    queue: VecDeque<Transaction>,
    /// сет прошедших транзакций
    seen: HashSet<Hash32Type>,
    /// порядок хэшей, чтобы чистить seen
    seen_order: VecDeque<Hash32Type>,
}

impl MemPool {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            seen: HashSet::new(),
            seen_order: VecDeque::new(),
        }
    }

    pub fn push(&mut self, transaction: Transaction) -> bool {
        if self.queue.len() >= MAX_MEMPOOL {
            return false;
        }

        let transaction_hash = transaction.hash();
        if self.seen.contains(&transaction_hash) {
            return false;
        }

        self.seen.insert(transaction_hash);
        self.seen_order.push_back(transaction_hash);
        self.queue.push_back(transaction);

        // очистка seed, прощай ООМ
        while self.seen.len() > MAX_SEEN {
            let Some(old) = self.seen_order.pop_front() else { break };
            self.seen.remove(&old);
        }

        true
    }

    pub fn pop_many(&mut self, count: usize) -> Vec<Transaction> {
        let n = count.min(self.queue.len());
        self.queue.drain(..n).collect()
    }

    pub fn remove_included(&mut self, included: &[Transaction]) {
        if included.is_empty() || self.queue.is_empty() {
            return;
        }

        let mut included_hashes: HashSet<Hash32Type> = HashSet::with_capacity(included.len());
        for tx in included {
            included_hashes.insert(tx.hash());
        }

        let mut new_q = VecDeque::with_capacity(self.queue.len());
        while let Some(tx) = self.queue.pop_front() {
            if !included_hashes.contains(&tx.hash()) {
                new_q.push_back(tx);
            }
        }
        self.queue = new_q;
    }
}
