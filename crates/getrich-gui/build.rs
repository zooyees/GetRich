fn main() {
    #[cfg(windows)]
    embed_icon();
}

#[cfg(windows)]
fn embed_icon() {
    let icon = std::path::Path::new("../../icon/GetRich.ico");
    let icon_alt = std::path::Path::new("../../Icon/GetRich.ico");
    let path = if icon.is_file() {
        icon
    } else if icon_alt.is_file() {
        icon_alt
    } else {
        println!("cargo:warning=GetRich.ico not found; PE icon not embedded");
        return;
    };
    let mut res = winresource::WindowsResource::new();
    res.set_icon(path.to_str().unwrap_or(""));
    res.set("ProductName", "GetRich");
    res.set("FileDescription", "GetRich stock workstation");
    res.set("LegalCopyright", "Yu Zhao");
    if let Err(e) = res.compile() {
        println!("cargo:warning=Failed to embed icon: {e}");
    }
    println!("cargo:rerun-if-changed={}", path.display());
}
