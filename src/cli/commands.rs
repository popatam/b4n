use super::args::{parse_args, pubkey_from_seed};
use crate::blockchain::consensus::{Ed25519Verifier, PoAConsensus, PoAConsensusConfig, PoAConsensusState, Validator};
use crate::blockchain::{BlockChain, Transaction};
use crate::node::{Node, NodeMessage, spawn_admin_listener, spawn_node};
use crate::transport::{ProtocolMessage, connect_peer, spawn_tcp_listener};
use std::sync::mpsc::channel;
use std::thread;
use std::time::Duration;

pub fn run() {
    let args = parse_args();

    let validators: Vec<Validator> = args
        .validator_pubkeys
        .iter()
        .map(|&seed| Validator::new(pubkey_from_seed(seed)))
        .collect();

    let consensus_config = PoAConsensusConfig::new(validators, 10_000, 10_000, 100);
    if !consensus_config.validate_config() {
        eprintln!("invalid consensus config");
        std::process::exit(254);
    }

    // chain и консенсус
    let chain = BlockChain::new(1);
    let verifier = Ed25519Verifier;
    let state = PoAConsensusState::new(chain.get_height(), chain.get_round());
    let consensus = PoAConsensus::new(consensus_config.clone(), state, verifier).unwrap();

    // нода
    let node = Node::new(args.net_id, args.seed, chain, consensus);
    let (tx, join_handle) = spawn_node(node);

    // канал для входящих сетевых сообщений
    let (proto_tx, proto_rx) = channel::<ProtocolMessage>();
    let _listener_handle = spawn_tcp_listener(&args.listen, proto_tx);

    // мост: всё что пришло по сети -> в NodeMessage
    let tx_to_node = tx.clone();
    thread::spawn(move || {
        while let Ok(p) = proto_rx.recv() {
            let m = NodeMessage::from_net(&p);
            let _ = tx_to_node.send(m);
        }
    });

    // админко
    let admin_addr = "127.0.0.1:18000";
    let _ = spawn_admin_listener(admin_addr, tx.clone(), args.net_id);

    // connect peers
    for (peer_id, peer_addr) in args.peers {
        let _ = tx.send(NodeMessage::AddPeer {
            peer_id,
            sender: connect_peer(&peer_addr),
        });
    }

    let tx_clone = tx.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(1));
        let trx_id = 1_000_000u64 + args.net_id as u64;
        let _ = tx_clone.send(NodeMessage::Trx(Transaction::new(
            trx_id,
            0,
            0,
            "i'm alive!".to_string(),
        )));
    });

    let _ = join_handle.join();
}
