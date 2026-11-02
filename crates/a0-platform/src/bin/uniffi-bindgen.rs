//! In-workspace UniFFI bindgen CLI (UniFFI 0.31 pattern).
//! Built only via the `bindgen` feature (`required-features` in
//! Cargo.toml), which enables `uniffi/cli`: the bindgen version is
//! guaranteed to match the `uniffi` library version.
fn main() {
    uniffi::uniffi_bindgen_main();
}
