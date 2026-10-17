//! Generates the Swift bindings, header and module map; used by `scripts/build-core-apple.sh`.

fn main() {
    uniffi::uniffi_bindgen_swift()
}
