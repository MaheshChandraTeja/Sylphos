#[cfg(target_os = "windows")]
fn main() {
    let mut resource = winresource::WindowsResource::new();

    resource.set_icon("assets/icon.ico");
    resource.set("ProductName", "Sylphos");
    resource.set("FileDescription", "Sylphos Browser");
    resource.set("CompanyName", "Kairais Tech");
    resource.set("OriginalFilename", "sylphos.exe");

    if let Err(error) = resource.compile() {
        panic!("failed to compile Windows resources: {error}");
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {}
