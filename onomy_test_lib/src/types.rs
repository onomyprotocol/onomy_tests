// TODO until musli/issues/20 is fixed we have the structs here

use musli::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct IbcSide {
    pub chain_id: String,
    pub connection: String,
    pub transfer_channel: String,
    pub ics_channel: String,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct IbcPair {
    pub a: IbcSide,
    pub b: IbcSide,
}

// note: some other impls are in the "hermes" file
