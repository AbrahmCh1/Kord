//! Embeds the app icon in the Windows executable. Regenerate the icon with
//! `kord render-icons assets`.

fn main() {
    println!("cargo:rerun-if-changed=kord.rc");
    println!("cargo:rerun-if-changed=../../assets/kord.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // An icon is nice to have, not a reason to fail the build when no
    // resource compiler (rc.exe / windres) is installed.
    if let Err(err) = embed_resource::compile("kord.rc", embed_resource::NONE).manifest_optional() {
        println!("cargo:warning=building without an exe icon: {err}");
    }
}
