fn main() {
    #[cfg(not(feature = "uniffi"))]
    panic!("uniffi feature required for uniffi-bindgen");
    #[cfg(feature = "uniffi")]
    uniffi::uniffi_bindgen_main()

}
