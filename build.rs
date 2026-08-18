fn main() {
    println!("cargo:rerun-if-changed=assets/fy.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/fy.ico")
            .compile()
            .expect("failed to embed Windows application icon");
    }
}
