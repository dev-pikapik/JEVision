//! Generates the Swift bindings, header and module map; used by `scripts/build-mobile-apple.sh`.

fn main() {
    uniffi::uniffi_bindgen_swift()
}
