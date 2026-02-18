///// админко

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::sync::mpsc::Sender;
use std::thread;
use crate::blockchain::Transaction;
use super::node::NodeMessage;

pub fn spawn_admin_listener(bind_addr: &str, node_tx: Sender<NodeMessage>, from_id: u32) -> thread::JoinHandle<()> {
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

                        let trx = Transaction::new(next_trx_id, from_id as u64, 0, text);
                        next_trx_id = next_trx_id.saturating_add(1);

                        let _ = tx.send(NodeMessage::Trx(trx));
                        continue;
                    }
                }
            });
        }
    })
}