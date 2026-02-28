use super::ProtocolMessage;
use crate::blockchain::PubkeyType;
use std::convert::TryInto;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::thread;
use std::time::Duration;
/*
mpsc почти каналы как в гошке (там mpmc), найти норм разбор сравнение FIXME

// go
ch := make(chan Message)
go func() {
    ch <- msg
}()
msg := <-ch

// rust  https://doc.rust-lang.org/book/ch16-02-message-passing.html
let (tx, rx) = channel::<Message>();
std::thread::spawn(move || {
    tx.send(msg).unwrap();
});
let msg = rx.recv().unwrap()

*/

#[derive(Debug)]
pub enum TransportEvent {
    /// сообщение от конкретного peer (peer_id берётся из handshake)
    Message { peer_id: u32, msg: ProtocolMessage },

    /// соединение установлено (можно добавить peer в ноду и использовать sender для отправки)
    PeerConnected {
        peer_id: u32,
        sender: Sender<ProtocolMessage>,
    },

    /// соединение умерло (можно чистить peers)
    PeerDisconnected { peer_id: u32 },
}

fn write_frame(stream: &mut TcpStream, payload: &[u8]) -> std::io::Result<()> {
    let len_u32: u32 = payload
        .len()
        .try_into()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "frame too big"))?;

    stream.write_all(&len_u32.to_be_bytes())?;
    stream.write_all(payload)?;
    stream.flush()?;
    Ok(())
}

fn read_frame(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;

    // тупая защита от ООМ
    const MAX_FRAME: usize = 16 * 1024 * 1024; // 16MB
    if len > MAX_FRAME {
        let error_msg = format!("frame bigger then {} bytes", MAX_FRAME);
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, error_msg));
    }

    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    Ok(buf)
}

fn write_msg(stream: &mut TcpStream, msg: &ProtocolMessage) -> std::io::Result<()> {
    let payload = postcard::to_stdvec(msg)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "serialize failed"))?;
    write_frame(stream, &payload)
}

fn read_msg(stream: &mut TcpStream) -> std::io::Result<ProtocolMessage> {
    let frame = read_frame(stream)?;
    postcard::from_bytes::<ProtocolMessage>(&frame)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "deserialize failed"))
}

/// Создаёт двунаправленную обработку stream:
///  writer: читает rx и пишет в stream
///  reader: читает из stream и шлёт TransportEvent::Message
fn spawn_peer_io(mut stream: TcpStream, peer_id: u32, tx_events: Sender<TransportEvent>) -> Sender<ProtocolMessage> {
    let (tx_out, rx_out) = channel::<ProtocolMessage>();
    let mut stream_w = stream.try_clone().expect("failed to clone tcpstream for writer");

    let disconnect_sent = Arc::new(AtomicBool::new(false));

    let send_disconnect = {
        let tx_events = tx_events.clone();
        let disconnect_sent = Arc::clone(&disconnect_sent);
        move || {
            if !disconnect_sent.swap(true, Ordering::SeqCst) {
                let _ = tx_events.send(TransportEvent::PeerDisconnected { peer_id });
            }
        }
    };

    // writer loop
    let tx_events_w = tx_events.clone();
    thread::spawn(move || {
        loop {
            let msg = match rx_out.recv() {
                Ok(m) => m,
                Err(_) => {
                    // sender дропнут, PeerDisconnected тут быть не должно
                    return;
                }
            };

            if let Err(e) = write_msg(&mut stream_w, &msg) {
                eprintln!("[transport] peer {} writer error: {}", peer_id, e);
                let _ = tx_events_w.send(TransportEvent::PeerDisconnected { peer_id });
                return;
            }
        }
    });

    // reader loop
    thread::spawn(move || {
        loop {
            match read_msg(&mut stream) {
                Ok(msg) => {
                    let _ = tx_events.send(TransportEvent::Message { peer_id, msg });
                }
                Err(e) => {
                    if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) {
                        continue;
                    }
                    eprintln!("[transport] peer {} reader error: {}", peer_id, e);
                    send_disconnect();
                    break;
                }
            }
        }
    });

    tx_out
}

/// listener принимает входящие соединения
///  первое сообщение должно быть Hello { peer_id, pubkey }.
pub fn spawn_tcp_listener(
    bind_addr: &str,
    self_peer_id: u32,
    _self_pubkey: PubkeyType,
    tx_events: Sender<TransportEvent>,
) -> thread::JoinHandle<()> {
    let addr = bind_addr.to_string();

    thread::spawn(move || {
        let listener = TcpListener::bind(&addr).expect("failed to bind tcp listener");

        for incoming in listener.incoming() {
            let mut stream = match incoming {
                Ok(s) => s,
                Err(_) => continue,
            };

            let _ = stream.set_nodelay(true);
            let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));

            let tx_events2 = tx_events.clone();

            thread::spawn(move || {
                // handshake ждём Hello
                let hello = match read_msg(&mut stream) {
                    Ok(m) => m,
                    Err(_) => return,
                };

                let (peer_id, _peer_pubkey) = match hello {
                    ProtocolMessage::Hello { peer_id, pubkey } => (peer_id, pubkey),
                    _ => return,
                };

                // отвечаем ack
                let _ = write_msg(&mut stream, &ProtocolMessage::HelloAck { peer_id: self_peer_id });

                // поднимаем io loops
                let sender = spawn_peer_io(stream, peer_id, tx_events2.clone());

                // сообщаем наверх что peer подключился и у нас есть sender
                let _ = tx_events2.send(TransportEvent::PeerConnected { peer_id, sender });
            });
        }
    })
}

/// Outbound connect
/// reconnect с экспоненциальным backoff до 5 секунд
/// handshake Hello -> HelloAck
pub fn connect_peer(
    peer_addr: &str,
    expected_peer_id: u32,
    self_peer_id: u32,
    self_pubkey: PubkeyType,
    tx_events: Sender<TransportEvent>,
) -> Sender<ProtocolMessage> {
    let addr = peer_addr.to_string();

    // sender который вернём сразу чтобы node мог слать даже пока reconnect
    let (tx_out, rx_out) = channel::<ProtocolMessage>();
    let tx_out_loop = tx_out.clone();

    thread::spawn(move || {
        let retry_sleep = Duration::from_millis(200);

        // reconnect loop
        loop {
            let mut stream = loop {
                match TcpStream::connect(&addr) {
                    Ok(s) => {
                        let _ = s.set_nodelay(true);
                        let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
                        let _ = s.set_write_timeout(Some(Duration::from_secs(10)));
                        break s;
                    }
                    Err(_) => {
                        thread::sleep(retry_sleep);
                        continue;
                    }
                }
            };

            // handshake send Hello
            let hello_msg = ProtocolMessage::Hello {
                peer_id: self_peer_id,
                pubkey: self_pubkey,
            };
            if let Err(e) = write_msg(&mut stream, &hello_msg) {
                eprintln!("[transport] peer {} handshake write error: {}", expected_peer_id, e);
                thread::sleep(retry_sleep);
                continue;
            }

            // handshake expect HelloAck
            let ack = match read_msg(&mut stream) {
                Ok(m) => m,
                Err(e) => {
                    if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) {
                        // считаем коннект неуспешным
                    }
                    eprintln!("[transport] peer {} handshake read error: {}", expected_peer_id, e);
                    thread::sleep(retry_sleep);
                    continue;
                }
            };

            let remote_id = match ack {
                ProtocolMessage::HelloAck { peer_id } => peer_id,
                _ => {
                    eprintln!("[transport] peer {} bad handshake ack", expected_peer_id);
                    thread::sleep(retry_sleep);
                    continue;
                }
            };

            if remote_id != expected_peer_id {
                eprintln!(
                    "[transport] peer {} handshake mismatch remote_id={}",
                    expected_peer_id, remote_id
                );
                thread::sleep(retry_sleep);
                continue;
            }

            // guard 'disconnect only once' для этого коннекта
            let disconnect_sent = Arc::new(AtomicBool::new(false));
            let send_disconnect = {
                let tx_events = tx_events.clone();
                let disconnect_sent = Arc::clone(&disconnect_sent);
                move || {
                    if !disconnect_sent.swap(true, Ordering::SeqCst) {
                        let _ = tx_events.send(TransportEvent::PeerDisconnected {
                            peer_id: expected_peer_id,
                        });
                    }
                }
            };

            // reader loop (читает из stream_r и шлёт Message)
            let mut stream_r = match stream.try_clone() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[transport] peer {} stream clone error: {}", expected_peer_id, e);
                    thread::sleep(retry_sleep);
                    continue;
                }
            };

            let tx_events_r = tx_events.clone();
            let send_disconnect_r = send_disconnect.clone();
            thread::spawn(move || {
                loop {
                    match read_msg(&mut stream_r) {
                        Ok(msg) => {
                            let _ = tx_events_r.send(TransportEvent::Message {
                                peer_id: expected_peer_id,
                                msg,
                            });
                        }
                        Err(e) => {
                            if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) {
                                continue;
                            }
                            eprintln!("[transport] peer {} reader error: {}", expected_peer_id, e);
                            send_disconnect_r();
                            break;
                        }
                    }
                }
            });

            // сообщаем наверх что можно использовать sender на этого peer
            let _ = tx_events.send(TransportEvent::PeerConnected {
                peer_id: expected_peer_id,
                sender: tx_out_loop.clone(),
            });

            // writer loop:
            //  rx_out закрыт -> завершаем весь поток
            //  ошибка записи -> PeerDisconnected (ОДИН РАЗ) + переподключаемся
            let send_disconnect_w = send_disconnect.clone();
            loop {
                let msg = match rx_out.recv() {
                    Ok(m) => m,
                    Err(_) => return, // больше некому слать
                };

                if let Err(e) = write_msg(&mut stream, &msg) {
                    eprintln!("[transport] peer {} writer error: {}", expected_peer_id, e);
                    send_disconnect_w();
                    break; // reconnect!
                }
            }

            thread::sleep(retry_sleep);
        }
    });

    tx_out
}
