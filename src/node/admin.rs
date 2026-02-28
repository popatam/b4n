use super::node::NodeMessage;
use crate::blockchain::Transaction;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    mpsc::Sender,
};
use std::thread;

pub fn spawn_admin_listener(bind_addr: &str, node_tx: Sender<NodeMessage>, from_id: u32) -> thread::JoinHandle<()> {
    // тут бы какой нить axum больше подошёл бы
    let addr = bind_addr.to_string();
    let global_next_trx_id = Arc::new(AtomicU64::new(1));

    thread::spawn(move || {
        let listener = TcpListener::bind(&addr).expect("failed to bind admin listener");

        for incoming in listener.incoming() {
            let stream = match incoming {
                Ok(s) => s,
                Err(_) => continue,
            };

            let tx = node_tx.clone();
            let global_next_trx_id = Arc::clone(&global_next_trx_id); // иначе хрень

            thread::spawn(move || {
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
                        let _ = tx.send(NodeMessage::DebugPrint);
                        continue;
                    }

                    if cmd == "stop" {
                        let _ = tx.send(NodeMessage::Stop);
                        break;
                    }

                    if let Some(rest) = cmd.strip_prefix("trx ") {
                        let text = rest.trim().to_string();
                        if text.is_empty() {
                            continue;
                        }

                        let id = global_next_trx_id.fetch_add(1, Ordering::Relaxed);
                        let trx = Transaction::new(id, from_id as u64, 0, text);

                        let _ = tx.send(NodeMessage::LocalTrx(trx));
                        continue;
                    }
                }
            });
        }
    })
}
