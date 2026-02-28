#[derive(Debug)]
pub enum BlockError {
    GenesisNotAllowedHere,
    InvalidIndex { expected: u64, got: u64 },
    InvalidPrevHash,
    InvalidMerkleRoot,
    InvalidProposer { expected: u32, got: u32 },
    InvalidSignature,
}

#[derive(Debug)]
pub enum SignError {
    NotValidator,
}
