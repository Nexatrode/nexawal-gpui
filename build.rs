fn main() {
    println!("cargo:rerun-if-changed=assets/nexawal.ico");
    println!("cargo:rerun-if-changed=assets/l10n.json");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/nexawal.ico");
        resource
            .compile()
            .expect("failed to embed the NexaWal Windows icon");
    }
}
