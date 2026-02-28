use super::args::{parse_args, pubkey_from_seed};
use crate::blockchain::consensus::{Ed25519Verifier, PoAConsensus, PoAConsensusConfig, PoAConsensusState, Validator};
use crate::blockchain::{BlockChain, Transaction};
use crate::node::{Node, NodeMessage, spawn_admin_listener, spawn_node};
use crate::transport::{TransportEvent, connect_peer, spawn_tcp_listener};
use std::sync::mpsc::channel;
use std::thread;
use std::time::Duration;

pub fn run() {
    let args = parse_args();

    let validators: Vec<Validator> = args
        .validator_pubkeys
        .iter()
        .map(|&pubkey| Validator::new(pubkey))
        .collect();

    let consensus_config = PoAConsensusConfig::new(validators, 3_000, 10_000, 100); // вынести куда нибудь константы
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
    let node = Node::new(args.node_id, args.seed, chain, consensus);
    let (tx, join_handle) = spawn_node(node);

    // канал для входящих сетевых сообщений
    let (ev_tx, ev_rx) = channel::<TransportEvent>();

    let self_pubkey = pubkey_from_seed(args.seed);
    let _listener_handle = spawn_tcp_listener(&args.listen, args.node_id, self_pubkey, ev_tx.clone());

    // мост: всё что пришло по сети -> в NodeMessage
    let tx_to_node = tx.clone();
    thread::spawn(move || {
        while let Ok(ev) = ev_rx.recv() {
            match ev {
                TransportEvent::PeerConnected { peer_id, sender } => {
                    let _ = tx_to_node.send(NodeMessage::AddPeer { peer_id, sender });
                }
                TransportEvent::PeerDisconnected { peer_id } => {
                    let _ = tx_to_node.send(NodeMessage::RemovePeer { peer_id });
                }
                TransportEvent::Message { peer_id, msg } => {
                    let _ = tx_to_node.send(NodeMessage::Net { peer_id, msg });
                }
            }
        }
    });

    // админко
    let _ = spawn_admin_listener(&args.admin_listen, tx.clone(), args.node_id);

    // connect peers
    for (peer_id, peer_addr) in args.peers {
        let _sender = connect_peer(&peer_addr, peer_id, args.node_id, self_pubkey, ev_tx.clone());
        // sender сам прилетит в PeerConnected через transport
    }

    // тестовая стартовая транзакция
    let tx_clone = tx.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(1));
        let trx_id = 1_000_000u64 + args.node_id as u64;
        let _ = tx_clone.send(NodeMessage::LocalTrx(Transaction::new(
            trx_id,
            0,
            0,
            "i'm alive!".to_string(),
        )));
    });

    let _ = join_handle.join();
}
