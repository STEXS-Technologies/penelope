use std::{env, error::Error, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let guide =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR")?).join("../../docs/getting-started.md");
    println!("cargo:rerun-if-changed={}", guide.display());
    let text = fs::read_to_string(guide)?.replace("\r\n", "\n");
    let mut examples = text.split("```rust\n").skip(1);
    let (source, _) = examples
        .next()
        .and_then(|block| block.split_once("\n```"))
        .ok_or("the getting-started guide must contain a complete Rust example")?;
    if examples.next().is_some() {
        return Err("the getting-started guide must contain exactly one Rust example".into());
    }
    fs::write(
        PathBuf::from(env::var("OUT_DIR")?).join("first_machine.rs"),
        source,
    )?;
    Ok(())
}
