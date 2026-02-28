use crate::blockchain::PubkeyType;
use ed25519_dalek::SigningKey;
use getrandom::fill;
use std::env;

pub fn pubkey_from_seed(seed: [u8; 32]) -> PubkeyType {
    let sk = SigningKey::from_bytes(&seed);
    sk.verifying_key().to_bytes()
}

#[derive(Debug)]
pub(crate) struct CliArgs {
    /// id ноды, вычисляет из seed
    pub(crate) node_id: u32,
    /// host:port
    pub(crate) listen: String,
    /// host:port админки
    pub(crate) admin_listen: String,
    /// приватный ключ (вернее то из чего он вычисляется)
    pub(crate) seed: [u8; 32],
    /// открытые ключи валидаторов
    pub(crate) validator_pubkeys: Vec<[u8; 32]>,
    /// соседи
    pub(crate) peers: Vec<(u32, String)>, // [(peer_id, "host:port")]
}

fn print_usage_and_exit() -> ! {
    eprintln!(
        "Usage:
  --gen-seed                генерация сида с последующим выходом
  --listen <ip:port>        tcp bind addr, пример: 0.0.0.0:7001
  --seed <hex64>            приватный ключ (32 bytes hex)
  --validator-pubkey <hex64>  публичный ключ валидатора (может быть несколько)
  --peer <id@ip:port>       сосед в формате 2@10.0.0.12:7001 (может быть несколько)

Пример
  Node3:
    --listen 0.0.0.0:7001 --seed <hex64_3> \\
    --validator-pubkey <hex64_1> --validator-pubkey <hex64_2> --validator-pubkey <hex64_3> \\
    --peer 1@10.0.0.11:7001 --peer 2@10.0.0.12:7001
"
    );
    std::process::exit(2);
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn hex_to_32(s: &str) -> Result<[u8; 32], String> {
    let bytes = s.as_bytes();
    if bytes.len() != 64 {
        return Err(format!("seed must be 64 hex chars, got {}", bytes.len()));
    }

    let mut out = [0u8; 32];
    for i in 0..32 {
        let hi = hex_val(bytes[2 * i]).ok_or_else(|| format!("invalid hex at pos {}", 2 * i))?;
        let lo = hex_val(bytes[2 * i + 1]).ok_or_else(|| format!("invalid hex at pos {}", 2 * i + 1))?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    const LOOKUP_TABLE: &[u8; 16] = b"0123456789abcdef";
    let mut out = Vec::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(LOOKUP_TABLE[(b >> 4) as usize]);
        out.push(LOOKUP_TABLE[(b & 0x0f) as usize]);
    }
    String::from_utf8(out).expect("hex is valid utf8")
}

// генерирует сид, ключ из него и id из него же
fn gen_seed_and_print() -> ! {
    let mut seed = [0u8; 32];
    fill(&mut seed).expect("getrandom failed");

    let pubkey = pubkey_from_seed(seed);
    let node_id = u32::from_be_bytes([seed[0], seed[1], seed[2], seed[3]]);

    println!("seed={}", bytes_to_hex(&seed));
    println!("pubkey={}", bytes_to_hex(&pubkey));
    println!("node_id={}", node_id);

    std::process::exit(0);
}

fn parse_peer_spec(s: &str) -> Result<(u32, String), String> {
    //  формат "2@10.0.0.12:7001"
    let Some((id_str, addr)) = s.split_once('@') else {
        return Err("peer must be in format <id@ip:port>".to_string());
    };

    let id: u32 = id_str.parse().map_err(|_| format!("invalid peer id '{}'", id_str))?;

    if addr.trim().is_empty() {
        return Err("peer addr is empty".to_string());
    }

    Ok((id, addr.to_string()))
}

pub(crate) fn parse_args() -> CliArgs {
    let mut it = env::args().skip(1);

    let mut listen: Option<String> = None;
    let mut seed: Option<[u8; 32]> = None;
    let mut validator_pubkeys: Vec<[u8; 32]> = Vec::new();
    let mut peers: Vec<(u32, String)> = Vec::new();
    let mut gen_seed: bool = false;
    let mut admin_listen: Option<String> = None;

    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--gen-seed" => {
                gen_seed = true;
            }
            "--admin" => {
                let v = it.next().unwrap_or_else(|| print_usage_and_exit());
                admin_listen = Some(v);
            }
            "--listen" => {
                let v = it.next().unwrap_or_else(|| print_usage_and_exit());
                listen = Some(v);
            }
            "--seed" => {
                let v = it.next().unwrap_or_else(|| print_usage_and_exit());
                seed = Some(hex_to_32(&v).unwrap_or_else(|_| print_usage_and_exit()));
            }
            "--validator-pubkey" => {
                let v = it.next().unwrap_or_else(|| print_usage_and_exit());
                let vs = hex_to_32(&v).unwrap_or_else(|_| print_usage_and_exit());
                validator_pubkeys.push(vs);
            }
            "--peer" => {
                let v = it.next().unwrap_or_else(|| print_usage_and_exit());
                let p = parse_peer_spec(&v).unwrap_or_else(|_| print_usage_and_exit());
                peers.push(p);
            }
            "--help" | "-h" => print_usage_and_exit(),
            _ => {
                eprintln!("unknown arg: {arg}");
                print_usage_and_exit();
            }
        }
    }

    if gen_seed {
        gen_seed_and_print();
    }

    let listen = listen.unwrap_or_else(|| print_usage_and_exit());
    let seed = seed.unwrap_or_else(|| print_usage_and_exit());
    let node_id = u32::from_be_bytes(seed[0..4].try_into().unwrap()); // тут безопасно, т.к. seed уже распаковался
    let admin_listen = admin_listen.unwrap_or_else(|| print_usage_and_exit());

    if validator_pubkeys.is_empty() {
        eprintln!("at least one --validator-pubkey is required");
        print_usage_and_exit();
    }

    // peer_id не должен совпадать с собой
    for (pid, _) in &peers {
        if *pid == node_id {
            eprintln!("peer id must not be equal to own node_id ({node_id})");
            print_usage_and_exit();
        }
    }

    CliArgs {
        node_id,
        listen,
        admin_listen,
        seed,
        validator_pubkeys,
        peers,
    }
}
