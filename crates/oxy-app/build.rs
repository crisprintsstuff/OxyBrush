//! Windows only: embed the application icon and version information in
//! `oxybrush.exe` (shown by Explorer, the taskbar and the installer).
//! On other targets this does nothing.

fn main() {
    println!("cargo:rerun-if-changed=../../packaging/icons/oxybrush.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../packaging/icons/oxybrush.ico")
        .set("ProductName", "OxyBrush")
        .set("FileDescription", "OxyBrush digital sculpting")
        .set("LegalCopyright", "The OxyBrush Contributors. MIT OR Apache-2.0");
    // A missing resource compiler (e.g. cross-compiling without windres)
    // must not break the build: the exe just has no icon.
    if let Err(e) = res.compile() {
        println!("cargo:warning=could not embed Windows icon/version resource: {e}");
    }
}
