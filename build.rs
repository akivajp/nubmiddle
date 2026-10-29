//! Embeds the icons (assets/nubmiddle.rc) into the Windows exe. Does nothing for other targets.

fn main() {
    println!("cargo:rerun-if-changed=assets/nubmiddle.rc");
    println!("cargo:rerun-if-changed=assets/nubmiddle.ico");
    println!("cargo:rerun-if-changed=assets/nubmiddle-paused.ico");
    embed_resource::compile("assets/nubmiddle.rc", embed_resource::NONE)
        .manifest_optional()
        .unwrap();
}
