fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/icon.ico");
    resource.set("ProductName", "ApolloLauncher");
    resource.set("FileDescription", "ApolloLauncher app launcher");
    if let Err(error) = resource.compile() {
        println!("cargo:warning=could not embed the app icon: {error}");
    }
}
