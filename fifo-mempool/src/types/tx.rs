use prost::bytes::Bytes;

// Placeholder for transaction hash type
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TxHash(pub Bytes);

impl std::fmt::Display for TxHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TxHash({:02x?})", &self.0)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RawTx(pub Bytes);

impl RawTx {
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }
}
