//! Compiles the window mark into the Windows executable as its icon resource.
//!
//! The `.ico` is generated here rather than committed, from the same [`icon`] module the running
//! window draws itself from. That is the whole point: a committed icon file is a second copy of
//! the mark, and a second copy is a thing that can quietly stop matching the first. `include!`
//! rather than a dependency on our own library, because a build script cannot depend on the crate
//! it is building — and `icon.rs` is plain `std`, which is what makes that legal.
//!
//! Windows only. It is the one platform that wants the icon inside the executable; X11 is told at
//! runtime, and macOS reads it from a bundle the release workflow assembles.
//!
//! `cfg(windows)` here is the *host*, not the target, because a build script runs on the host.
//! Every build that matters is native — a runner per platform, which is what the release workflow
//! is for — so the two agree. Cross-compiling to Windows from elsewhere would silently skip this,
//! and the icon going missing is the symptom to remember it by.

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(windows)]
    windows_icon();
}

#[cfg(windows)]
fn windows_icon() {
    mod icon {
        include!("src/icon.rs");
    }

    // 16 through 256: Explorer's list, tiles and preview sizes. Anything absent gets scaled from
    // a neighbour, and a mark this simple survives that better than most, but the small ones are
    // where a rounded tile and a thin ring show their edges.
    const SIZES: &[u32] = &[16, 32, 48, 64, 128, 256];

    let out = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    let path = std::path::Path::new(&out).join("manic-microbes.ico");
    std::fs::write(&path, icon::ico(SIZES)).expect("write the generated icon");

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon(path.to_str().expect("OUT_DIR is valid UTF-8"));
    resource.compile().expect("compile the Windows resource");
}
