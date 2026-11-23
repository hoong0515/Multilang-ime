// Shared by build.rs of tip, backend and candhost (include!): writes a Windows resource script with
// the version block (and the app icon) and compiles it with rc.exe through embed-resource.
#[allow(dead_code)]
fn embed_version_resource(description: &str, file_name: &str, is_dll: bool, with_icon: bool, extra_icons: &[(u16, &str)]) {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let mut nums: Vec<u16> = version.split(['.', '-', '+']).filter_map(|p| p.parse().ok()).collect();
    nums.resize(4, 0);
    let commas = format!("{},{},{},{}", nums[0], nums[1], nums[2], nums[3]);
    let res_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("res");
    let icon = res_dir.join("multilang.ico");
    println!("cargo::rerun-if-changed={}", icon.display());
    println!("cargo::rerun-if-changed={}", res_dir.join("build_res.rs").display());
    let mut extra = String::new();
    for (id, name) in extra_icons {
        let path = res_dir.join(name);
        println!("cargo::rerun-if-changed={}", path.display());
        extra.push_str(&format!("{id} ICON \"{}\"\n", path.display().to_string().replace('\\', "/")));
    }
    let icon_line = if with_icon {
        format!("1 ICON \"{}\"\n", icon.display().to_string().replace('\\', "/"))
    } else {
        String::new()
    };
    let rc = format!(
        r#"#include <winver.h>
{icon_line}{extra}VS_VERSION_INFO VERSIONINFO
 FILEVERSION {commas}
 PRODUCTVERSION {commas}
 FILEFLAGSMASK 0x3fL
 FILEFLAGS 0x0L
 FILEOS VOS_NT_WINDOWS32
 FILETYPE {ftype}
 FILESUBTYPE 0x0L
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "Multilang IME contributors"
      VALUE "FileDescription", "{description}"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "{file_name}"
      VALUE "LegalCopyright", "GPL-3.0-or-later"
      VALUE "OriginalFilename", "{file_name}"
      VALUE "ProductName", "Multilang IME"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        ftype = if is_dll { "VFT_DLL" } else { "VFT_APP" },
    );
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("version.rc");
    std::fs::write(&out, rc).unwrap();
    if is_dll {
        embed_resource::compile_for_everything(&out, embed_resource::NONE).manifest_required().unwrap();
    } else {
        embed_resource::compile(&out, embed_resource::NONE).manifest_required().unwrap();
    }
}
