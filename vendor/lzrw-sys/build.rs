fn main()
{
    println!("cargo:rerun-if-changed=lzrw/");

    cc::Build::new()
        .warnings(false)
        .files([
            "lzrw/fast-copy.c",
            "lzrw/lzrw1.c",
            "lzrw/lzrw1-a.c",
            "lzrw/lzrw2.c",
            "lzrw/lzrw3.c",
            "lzrw/lzrw3-a.c",
            "lzrw/lzrw5.c",
        ])
        .include("lzrw")
        .compile("lzrw");
}
