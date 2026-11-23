//! The command carries its own copy of the catalog so that the published
//! crate is whole. It has to stay the catalog of the repository.

#[test]
fn the_built_in_catalog_is_the_one_in_data() {
    let own: &[u8] = include_bytes!("../data/hip65.astc");
    let shared: &[u8] = include_bytes!("../../../data/hip65.astc");
    assert!(
        own == shared,
        "crates/asterope-cli/data/hip65.astc differs from data/hip65.astc"
    );
}
