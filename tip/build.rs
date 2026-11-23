include!("../res/build_res.rs");

fn main() {
    // icon resource 1 = index 0, which the TSF profile registration points at (input indicator icon)
    embed_version_resource("Multilang IME text service", "multilang_tip.dll", true, true);
}
