use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    println!("cargo:rerun-if-env-changed=FARCRI_LINK_SEARCH");
    if let Ok(link_search) = env::var("FARCRI_LINK_SEARCH") {
        println!("cargo:rustc-link-search={}", link_search);
    }
}
