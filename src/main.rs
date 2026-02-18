mod blockchain;
mod cli;
mod node;
mod transport;

/*
делаю proof of authority
nonce нужен только если proof of work

*/

fn main() {
    cli::run();
}
