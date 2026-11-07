//! Puts the program icon into the Windows executable. Does nothing on other systems.

fn main() {
    println!("cargo:rerun-if-changed=assets/vma.rc");
    println!("cargo:rerun-if-changed=assets/vma.ico");
    embed_resource::compile("assets/vma.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("couldn't embed the program icon");
}
