use target_build_utils::TargetInfo;

fn main() {
    let target = TargetInfo::new().expect("could not get target info");

    if target.target_os() == "macos" {
        let arch = if target.target_arch() == "x86_64" {
            "x86_64"
        } else {
            "arm64"
        };

        println!("cargo:rerun-if-changed=rubin");
        let dst = cmake::Config::new("rubin")
            .define("CMAKE_OSX_ARCHITECTURES", arch)
            .build();
        println!(
            "cargo:rustc-link-search=native={}",
            dst.join("lib").display()
        );
        println!("cargo:rustc-link-lib=static=rubin");

        println!("cargo:rerun-if-changed=lzo");
        let dst2 = cmake::Config::new("lzo")
            .define("CMAKE_OSX_ARCHITECTURES", arch)
            .build();
        println!(
            "cargo:rustc-link-search=native={}",
            dst2.join("lib").display()
        );
        println!("cargo:rustc-link-lib=static=lzo2");
    } else {
        println!("cargo:rerun-if-changed=rubin");

        let dst = cmake::build("rubin");
        println!(
            "cargo:rustc-link-search=native={}",
            dst.join("lib").display()
        );
        println!("cargo:rustc-link-lib=static=rubin");

        println!("cargo:rerun-if-changed=lzo");
        let dst2 = cmake::build("lzo");
        println!(
            "cargo:rustc-link-search=native={}",
            dst2.join("lib").display()
        );
        println!("cargo:rustc-link-lib=static=lzo2");
    }
}
