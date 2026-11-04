use serde::{Deserialize, Serialize};
use std::fmt;

/// RBI FETERS purpose codes relevant to digital service exporters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PurposeCode {
    /// Software consultancy/implementation.
    P0802,
    /// Data base / data processing charges.
    P0803,
    /// Off-site software exports (Non-STP registration + SOFTEX required).
    P0807,
    /// Business and management consultancy.
    P1006,
    /// Advance payment against physical-goods exports. FORBIDDEN for services:
    /// creates an unfulfillable EDPMS "ghost" export obligation.
    P0103,
    Other(String),
}

impl fmt::Display for PurposeCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PurposeCode::P0802 => write!(f, "P0802"),
            PurposeCode::P0803 => write!(f, "P0803"),
            PurposeCode::P0807 => write!(f, "P0807"),
            PurposeCode::P1006 => write!(f, "P1006"),
            PurposeCode::P0103 => write!(f, "P0103"),
            PurposeCode::Other(s) => write!(f, "{s}"),
        }
    }
}
