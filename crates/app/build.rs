//! On Windows the .exe carries the app's icon and its name and version (dsky-fbso.14): GPUI gives
//! its windows icon resource 1, and Explorer, the taskbar and the installer show it too.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../packaging/windows/dostoevsky.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../packaging/windows/dostoevsky.ico")
            .set("ProductName", "Dostoevsky")
            .set("FileDescription", "Dostoevsky")
            .set("LegalCopyright", "Copyright 2026 Dmitry Eibozhenko and the Dostoevsky contributors");
        res.compile().expect("the .exe's icon and version");
    }
}
