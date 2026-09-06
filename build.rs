fn main() {
    println!("cargo:rerun-if-changed=assets/chloevibes.ico");
    println!("cargo:rerun-if-changed=Cargo.toml");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/chloevibes.ico")
            .set("ProductName", "ChloeVibes")
            .set("FileDescription", "ChloeVibes music-driven haptics")
            .set("InternalName", "chloe-vibes")
            .set("OriginalFilename", "ChloeVibes-windows-x64.exe")
            .compile()
            .expect("Failed to compile the ChloeVibes Windows icon and version resources");
    }
}
