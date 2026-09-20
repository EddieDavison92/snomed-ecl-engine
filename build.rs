fn main() {
    #[cfg(feature = "unicode")]
    {
        let icu = pkg_config::Config::new()
            .atleast_version("72")
            .statik(true)
            .cargo_metadata(false)
            .probe("icu-i18n")
            .expect("Unicode term matching requires ICU4C development libraries and pkg-config");
        let mut build = cc::Build::new();
        build.file("src/text/icu.c");
        for path in &icu.include_paths {
            build.include(path);
        }
        build.warnings(true).compile("snomed_icu_search");
        for path in &icu.link_paths {
            println!("cargo:rustc-link-search=native={}", path.display());
        }
        for lib in &icu.libs {
            if lib.starts_with("icu") {
                println!("cargo:rustc-link-lib=static={lib}");
            } else {
                println!("cargo:rustc-link-lib={lib}");
            }
        }
        let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
        if target == "macos" {
            println!("cargo:rustc-link-lib=c++");
        } else if target != "windows" {
            println!("cargo:rustc-link-lib=stdc++");
        }
        println!("cargo:rustc-env=SNOMED_ICU_VERSION={}", icu.version);
        println!("cargo:rerun-if-changed=src/text/icu.c");
    }
}
