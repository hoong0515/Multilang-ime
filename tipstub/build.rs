include!("../res/build_res.rs");

fn main() {
    // icon resource 1 = index 0: the TSF profile registration points at this DLL (input method list icon)
    embed_version_resource("Multilang IME text service", "multilang_tip.dll", true, true, &[]);
}
