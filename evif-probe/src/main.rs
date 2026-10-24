//! `evif`: probe metrics, and inspect, verify and unpack EVIF files, with the
//! families and judges built into this crate.

fn main() -> ! {
    evif_probe::cli::main(&evif_probe::registry::Registry::builtin())
}
