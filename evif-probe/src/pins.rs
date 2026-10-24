//! Versions and revisions of the codecs and metrics this binary links.
//!
//! These go into every file's `knob` and `scor` chunks so a score can be
//! traced to the exact implementation that produced it. Keep them in step with
//! the `rev =` pins in `Cargo.toml`.

pub struct Pin {
    pub version: &'static str,
    pub commit: &'static str,
}

impl Pin {
    /// `name@<8-char commit>`, or `name@<version>` for a crates.io release.
    pub fn implementation(&self, name: &str) -> String {
        if self.commit == REGISTRY {
            format!("{name}@{}", self.version)
        } else {
            format!("{name}@{}", &self.commit[..8])
        }
    }
}

/// Marker commit for a dependency taken from crates.io at an exact version.
pub const REGISTRY: &str = "crates.io";

pub const ZENJPEG: Pin = Pin {
    version: "0.9.0",
    commit: "a2a128ab0bc36f01338225f61c285f462ae67ca6",
};
pub const ZENSIM: Pin = Pin {
    version: "0.3.0",
    commit: "2b84071712ef86bc95f7fdfc0b14b7ee14d2aaeb",
};
pub const FAST_SSIM2: Pin = Pin {
    version: "0.9.0",
    commit: "54fabca919230767ba0944cbec0a37d7eb3a2a7a",
};
pub const BUTTERAUGLI: Pin = Pin {
    version: "0.9.4",
    commit: "02b570b490dda0f2593aca99260ed34ab062d5b3",
};
pub const CORRUPTION_CORPUS: Pin = Pin {
    version: "0.1.0",
    commit: "ac980c295cb62c925d03d47990925d49c36eef18",
};
pub const ZENPNG: Pin = Pin {
    version: "0.2.0",
    commit: "2802f8ee9ae5444b61c4b8f9da51645365bb1fd5",
};
pub const ZENQUANT: Pin = Pin {
    version: "0.1.3",
    commit: REGISTRY,
};
