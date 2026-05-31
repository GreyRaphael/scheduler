use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=config.toml");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let binding = out_dir.clone();
    let target_dir = out_dir
        .ancestors()
        .find(|p| p.join("scheduler.exe").exists() || p.join("scheduler").exists())
        .unwrap_or(binding.parent().unwrap().parent().unwrap().parent().unwrap());

    if let Ok(contents) = fs::read_to_string("config.toml") {
        let dest = target_dir.join("config.toml");
        let _ = fs::write(&dest, contents);
    }
}
