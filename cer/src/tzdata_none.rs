//! Time zone data left out: built without the `tzdata` feature, the only
//! zone is UTC. See tzdata.rs and cer/tools/gen-tzdata.cjs.

pub fn version() -> String {
    String::from("none")
}

pub fn zones() -> String {
    String::new()
}
