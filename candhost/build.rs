include!("../res/build_res.rs");

// The candidate host runs with UIAccess so its topmost window can sit above the shell's search UI
// (a higher window band than normal apps). Windows only grants UIAccess to a signed exe installed in a
// secure location (Program Files), started through ShellExecute.
fn main() {
    embed_version_resource("Multilang IME candidate window", "MultilangIME-CandWin.exe", false, true);
    // shared drawing code (tip/src/candwin.rs) is compiled here without the host routing
    println!("cargo::rustc-check-cfg=cfg(candhost)");
    println!("cargo::rustc-cfg=candhost");
    println!("cargo::rerun-if-changed=../tip/src/candwin.rs");
    println!("cargo::rerun-if-env-changed=CANDHOST_NO_UIACCESS");
    println!("cargo::rustc-link-arg-bins=/MANIFEST:EMBED");
    // CANDHOST_NO_UIACCESS=1: test build that runs from anywhere unsigned (no UIAccess, so it stays
    // under the search UI; for checking drawing and messages only)
    let ui_access = if std::env::var_os("CANDHOST_NO_UIACCESS").is_some() { "false" } else { "true" };
    println!("cargo::rustc-link-arg-bins=/MANIFESTUAC:level='asInvoker' uiAccess='{ui_access}'");
}
