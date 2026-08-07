//! That `build.rs` can still include `src/icon.rs`.
//!
//! The build script generates the Windows `.ico` by pulling that file in with `include!`, because
//! a build script cannot depend on the crate it is building. `include!` is a macro expansion, and
//! an inner doc comment — `//!` — is illegal in one. So a perfectly ordinary edit to the top of
//! `icon.rs` compiles everywhere, passes every test, and then fails eight minutes into the
//! Windows job of a release, which is the worst place to learn it.
//!
//! It happened exactly once. This is why it will not happen twice: the same `include!`, run on
//! every platform, so the failure arrives on a laptop instead of on a runner.

mod icon {
    include!("../src/icon.rs");
}

#[test]
fn the_icon_module_still_includes_the_way_the_build_script_includes_it() {
    // Reaching these at all is the assertion — if the file grew an inner attribute this would not
    // compile. The values are checked anyway, because a test that only compiles reads like an
    // accident to whoever finds it next.
    assert_eq!(icon::rgba(16).len(), 16 * 16 * 4);

    let ico = icon::ico(&[16, 32]);
    assert_eq!(&ico[0..4], &[0, 0, 1, 0], "ICONDIR: reserved 0, type 1");
    assert_eq!(ico[4], 2, "two images in the directory");
}
