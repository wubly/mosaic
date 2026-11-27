//! Embeds the app icon and version info into `mosaic.exe` on Windows. Explorer, the taskbar and
//! the installer's shortcuts take the icon from the executable's resources, so without this an
//! installed Mosaic shows the blank default icon.

fn main() {
    println!("cargo:rerun-if-changed=../assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../assets/icon.ico")
        .set("ProductName", "Mosaic")
        .set("FileDescription", "Mosaic")
        .set("CompanyName", "Mosaic");
    if let Err(e) = res.compile() {
        // A missing resource compiler shouldn't break local builds; release builds have one.
        println!("cargo:warning=couldn't embed the Windows icon: {e}");
    }
}
