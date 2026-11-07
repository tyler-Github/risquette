//! Embeds the app icon and version info into the Windows executable.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/risquette.ico");
    println!("cargo:rerun-if-env-changed=RISQUETTE_UPDATE_URL");
    #[cfg(windows)]
    windows_resources("Risquette", "risquette.exe");
}

/// Write a resource script (icon as ID 1, which GPUI uses for the window, plus
/// version info) to OUT_DIR and compile it in.
#[cfg(windows)]
fn windows_resources(description: &str, filename: &str) {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let parts: Vec<u16> = version.split(['.', '-']).take(3).map(|p| p.parse().unwrap_or(0)).collect();
    let num = format!("{},{},{},0", parts.first().unwrap_or(&0), parts.get(1).unwrap_or(&0), parts.get(2).unwrap_or(&0));
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon = manifest.join("../../assets/risquette.ico").canonicalize().expect("assets/risquette.ico (run scripts/make_icon.py)");
    let icon = icon.to_string_lossy().replace("\\\\?\\", "").replace('\\', "\\\\");
    let rc = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {num}
PRODUCTVERSION {num}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Risquette contributors"
      VALUE "FileDescription", "{description}"
      VALUE "FileVersion", "{version}"
      VALUE "OriginalFilename", "{filename}"
      VALUE "ProductName", "Risquette"
      VALUE "ProductVersion", "{version}"
      VALUE "LegalCopyright", "Apache-2.0"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("app.rc");
    std::fs::write(&out, rc).unwrap();
    embed_resource::compile(&out, embed_resource::NONE).manifest_optional().unwrap();
}
