include!("../res/build_res.rs");

fn main() {
    embed_version_resource("Multilang IME backend", "backend.exe", false, true);
}
