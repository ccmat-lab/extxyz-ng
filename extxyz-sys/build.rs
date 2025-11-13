use bindgen::builder;

fn main() {
    // compile extxyz
    let mut build = cc::Build::new();
    build
        .include("./extxyz/libcleri/inc/")
        .include("./extxyz/libextxyz/")
        .include("./grammar-gen/")
        .file("./extxyz/libextxyz/extxyz.c")
        .file("./grammar-gen/extxyz_kv_grammar.c")
        .files([
            "./extxyz/libcleri/src/dup.c",
            "./extxyz/libcleri/src/children.c",
            "./extxyz/libcleri/src/choice.c",
            "./extxyz/libcleri/src/cleri.c",
            "./extxyz/libcleri/src/dup.c",
            "./extxyz/libcleri/src/expecting.c",
            "./extxyz/libcleri/src/grammar.c",
            "./extxyz/libcleri/src/keyword.c",
            "./extxyz/libcleri/src/kwcache.c",
            "./extxyz/libcleri/src/list.c",
            "./extxyz/libcleri/src/node.c",
            "./extxyz/libcleri/src/olist.c",
            "./extxyz/libcleri/src/optional.c",
            "./extxyz/libcleri/src/parse.c",
            "./extxyz/libcleri/src/prio.c",
            "./extxyz/libcleri/src/ref.c",
            "./extxyz/libcleri/src/regex.c",
            "./extxyz/libcleri/src/repeat.c",
            "./extxyz/libcleri/src/rule.c",
            "./extxyz/libcleri/src/sequence.c",
            "./extxyz/libcleri/src/this.c",
            "./extxyz/libcleri/src/token.c",
            "./extxyz/libcleri/src/tokens.c",
            "./extxyz/libcleri/src/version.c",
        ])
        .compile("extxyz");

    println!("cargo:rustc-link-lib=pcre2-8");

    // Configure and generate bindings.
    let bindings = builder()
        .header("./wrapper/extxyz_wrapper.h")
        .header("./grammar-gen/extxyz_kv_grammar.h")
        .clang_arg("-I./extxyz/libcleri/inc/")
        .blocklist_type("FILE")
        .raw_line("use libc::FILE;")
        .allowlist_function("compile_extxyz_kv_grammar")
        .allowlist_function("free_dict")
        .allowlist_function("print_dict")
        .allowlist_function("extxyz_.*")
        .generate()
        .expect("unable to generate bindings");

    bindings
        .write_to_file("./src/bindings.rs")
        .expect("Couldn't write bindings!");
}
