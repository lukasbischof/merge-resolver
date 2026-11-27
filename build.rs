//! Packs the grammars for syntax highlighting into one file that the
//! binary embeds: bat's collection plus the ones in `syntaxes/`.

use std::env;
use std::path::Path;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=syntaxes");
    let mut builder = two_face::syntax::extra_newlines().into_builder();
    builder
        .add_from_folder("syntaxes", true)
        .expect("the grammars in syntaxes/ load");
    let out = Path::new(&env::var_os("OUT_DIR").expect("OUT_DIR is set")).join("syntaxes.bin");
    syntect::dumps::dump_to_uncompressed_file(&builder.build(), out)
        .expect("the grammars can be written");
}
