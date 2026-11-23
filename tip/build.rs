include!("../res/build_res.rs");

fn main() {
    // icon resource 1 = index 0, which the TSF profile registration points at (input indicator icon)
    embed_version_resource("Multilang IME text service", "multilang_tip.dll", true, true, &[
        // input indicator: current language, auto (dots) / locked (padlock) -> tip/src/langbar.rs
        (101, "lang-ko-auto.ico"),
        (102, "lang-en-auto.ico"),
        (103, "lang-ja-auto.ico"),
        (111, "lang-ko-lock.ico"),
        (112, "lang-en-lock.ico"),
        (113, "lang-ja-lock.ico"),
    ]);
}
