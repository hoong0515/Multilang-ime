include!("../res/build_res.rs");

/// The stub's own version, not the package version: the installer leaves a stub with the same bytes in place
/// (apps keep it loaded), so the file must not change on every release. Bump this only when the stub code
/// itself changes (`tipstub/src`).
const STUB_VERSION: &str = "1.0.0";

fn main() {
    // icon resource 1 = index 0: the TSF profile registration points at this DLL (input method list icon)
    embed_version_resource_as(STUB_VERSION, "Multilang IME text service", "multilang_tip.dll", true, true, &[]);
    // reproducible link (no timestamp, deterministic debug GUID): an unchanged stub builds to the same bytes
    println!("cargo::rustc-link-arg=/Brepro");
}
