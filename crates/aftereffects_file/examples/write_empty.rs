//! Development probe, not the user-facing Tesseract conversion command.

use std::{env, fs::OpenOptions, io::Write, path::Path};

use aftereffects_file::writer::{CompositionSpec, write_empty_composition};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let [output, width, height, frames, name] = args.as_slice() else {
        return Err("usage: write_empty OUTPUT WIDTH HEIGHT DURATION_FRAMES NAME".into());
    };
    let spec = CompositionSpec {
        name: name.to_str().ok_or("name must be UTF-8")?.to_owned(),
        width: width.to_str().ok_or("width must be UTF-8")?.parse()?,
        height: height.to_str().ok_or("height must be UTF-8")?.parse()?,
        duration_frames: frames.to_str().ok_or("frames must be UTF-8")?.parse()?,
    };
    let bytes = write_empty_composition(&spec)?;
    // Fail on existing paths: this probe must never replace an AE-authored file.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    file.write_all(&bytes)?;
    println!(
        "Generated {} bytes at {}; Adobe compatibility requires a separate open test.",
        bytes.len(),
        Path::new(output).display()
    );
    Ok(())
}
