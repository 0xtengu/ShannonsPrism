use cmake::Config;
use std::env;
#[cfg(feature = "generate_binding")]
use std::path::PathBuf;

#[cfg(feature = "generate_binding")]
fn generate_bindings()
{
    const ALLOW_UNCONVENTIONALS: &'static str = "#![allow(non_upper_case_globals)]\n\
                                                 #![allow(non_camel_case_types)]\n\
                                                 #![allow(non_snake_case)]\n\
                                                 #![allow(improper_ctypes)]\n";

    let bindings = bindgen::Builder::default()
        .header("src/wrapper.h")
        .raw_line(ALLOW_UNCONVENTIONALS)
        .generate()
        .expect("Unable to generate bindings");

    let binding_target_path = PathBuf::new().join("src").join("lib.rs");

    bindings
        .write_to_file(binding_target_path)
        .expect("Could not write binding to the file at `src/lib.rs`");

    println!("cargo:info=Successfully generated binding.");
}

/// Returns an expected default library linking method based on OS or target.
///
/// For Windows, macOS and Linux with musl, it will be true, which denotes `static`.
/// For Linux without musl, it will be false, to denote `dynamic`.
///
/// Note that this is a helper function and may not be called if the `static` feature
/// is enabled or the environment variable `LIBLZHAM_STATIC` or `LZHAM_STATIC` is set.
fn default_library_linking() -> bool
{
    #[cfg(any(windows, target_os = "macos", target_env = "musl"))]
    {
        true
    }
    #[cfg(all(unix, target_env = "gnu"))]
    {
        false
    }
}

/// Checks if it is a static build.
///
/// It takes features and environment variables into account first. If none set, it returns
/// answer based on the OS and architecture.
fn is_static_build() -> bool
{
    if cfg!(feature = "static") && cfg!(feature = "dynamic")
    {
        default_library_linking()
    }
    else if cfg!(feature = "static")
        || env::var("LIBLZHAM_STATIC").is_ok()
        || env::var("LZHAM_STATIC").is_ok()
    {
        println!("cargo:info=Static feature or environment variable found.");

        true
    }
    else if cfg!(feature = "dynamic")
    {
        println!("cargo:info=Dynamic feature enabled.");

        false
    }
    else
    {
        println!("cargo:info=No feature or environment variable found, linking by default.");

        default_library_linking()
    }
}

fn main()
{
    #[cfg(feature = "generate_binding")]
    generate_bindings();

    match env::var("CARGO_CFG_TARGET_OS").as_deref()
    {
        Ok("macos") => println!("cargo:rustc-link-lib=c++"),
        Ok("windows") =>
        {}
        _ => println!("cargo:rustc-link-lib=stdc++"),
    }

    let is_static = is_static_build();

    let dst = if is_static
    {
        Config::new("lzham_codec")
            .define("BUILD_SHARED_LIBS", "OFF")
            .define("CMAKE_POLICY_VERSION_MINIMUM", "3.5")
            .build()
    }
    else
    {
        cmake::build("lzham_codec")
    };

    println!("cargo:rustc-link-search=native={}/lib", dst.display());

    let linking_text = if is_static { "static" } else { "dylib" };

    println!("cargo:rustc-link-lib={}=lzhamdecomp", linking_text);
    println!("cargo:rustc-link-lib={}=lzhamcomp", linking_text);
    println!("cargo:rustc-link-lib={}=lzhamdll", linking_text);
}
