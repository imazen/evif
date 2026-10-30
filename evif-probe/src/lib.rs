//! Stress families, metric judges and a matched-bitrate disagreement search
//! that write EVIF files. See the repository README for the method.

#![forbid(unsafe_code)]

pub mod cli;
pub mod family;
pub mod image;
pub mod judge;
pub mod pins;
pub mod registry;
pub mod rendition;
pub mod rng;
pub mod search;
pub mod write;

/// Errors here are reported to a person running the CLI, so a message is
/// enough.
pub type Res<T> = Result<T, String>;
#[cfg(feature = "adversarial")]
mod adversarial;
#[cfg(feature = "adversarial")]
mod adversarial_judges;
#[cfg(feature = "trade")]
mod trade;
#[cfg(feature = "trade")]
mod trade_judges;
mod wire_cli;
