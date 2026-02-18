use super::ProtocolMessage;
use std::convert::TryInto;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
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

pub fn spawn_tcp_listener(bind_addr: &str, node_in_tx: Sender<ProtocolMessage>) -> thread::JoinHandle<()> {
    let addr = bind_addr.to_string();

    thread::spawn(move || {
        let listener = TcpListener::bind(&addr).expect("failed to bind tcp listener");
        for incoming in listener.incoming() {
            match incoming {
                Ok(mut stream) => {
                    let tx = node_in_tx.clone();
                    let _ = stream.set_nodelay(true);
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(10))); // вынести в конфиг
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));

                    thread::spawn(move || {
                        loop {
                            let frame = match read_frame(&mut stream) {
                                Ok(f) => f,
                                Err(_) => break, // сдох коннект или EOF или ещё чего
                            };

                            let wire: ProtocolMessage = match postcard::from_bytes(&frame) {
                                Ok(m) => m,
                                Err(_) => continue, // мусор
                            };

                            let _ = tx.send(wire);
                        }
                    });
                }
                Err(_) => {
                    // тут вероятно должна быть какая то логика
                    continue;
                }
            }
        }
    })
}

pub fn connect_peer(peer_addr: &str) -> Sender<ProtocolMessage> {
    let (tx, rx) = channel::<ProtocolMessage>();
    let addr = peer_addr.to_string();

    thread::spawn(move || {
        // reconnect loop вместо backoff, разобраться как тут backoff носят
        let mut stream = loop {
            match TcpStream::connect(&addr) {
                Ok(s) => {
                    let _ = s.set_nodelay(true);
                    let _ = s.set_read_timeout(Some(Duration::from_secs(10))); // вынести в конфиг
                    let _ = s.set_write_timeout(Some(Duration::from_secs(10)));
                    break s;
                }
                Err(_) => {
                    thread::sleep(Duration::from_millis(200));
                    continue;
                }
            }
        };

        while let Ok(wire) = rx.recv() {
            let payload = match postcard::to_stdvec(&wire) {
                Ok(p) => p,
                Err(_) => continue,
            };

            if write_frame(&mut stream, &payload).is_err() {
                // если коннект умер пытаемся переподключиться и продолжить
                stream = loop {
                    match TcpStream::connect(&addr) {
                        Ok(s) => {
                            let _ = s.set_nodelay(true);
                            let _ = s.set_read_timeout(Some(Duration::from_secs(10))); // вынести в конфиг
                            let _ = s.set_write_timeout(Some(Duration::from_secs(10)));
                            break s;
                        }
                        Err(_) => {
                            thread::sleep(Duration::from_millis(200));
                            continue;
                        }
                    }
                };
            }
        }
    });

    tx
}
