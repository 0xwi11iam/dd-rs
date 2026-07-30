/// Build script: compiles the C conversion helpers (EBCDIC/ASCII/IBM tables and routines)
/// into a static library that gets linked into the Rust binary.

fn main() {
    // Compile the C conversion support library
    cc::Build::new()
        .file("c_src/ebcdic_tables.c")
        .file("c_src/conv_helpers.c")
        .include("c_src")
        .opt_level(3)
        .warnings(true)
        .compile("dd_rs_conv");

    // Re-run build.rs if any of these change
    println!("cargo:rerun-if-changed=c_src/ebcdic_tables.c");
    println!("cargo:rerun-if-changed=c_src/conv_helpers.c");
    println!("cargo:rerun-if-changed=c_src/ebcdic_tables.h");
    println!("cargo:rerun-if-changed=c_src/conv_helpers.h");
    println!("cargo:rerun-if-changed=build.rs");
}
