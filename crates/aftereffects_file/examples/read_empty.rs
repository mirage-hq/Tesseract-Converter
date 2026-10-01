//! Development-only metadata probe for files reopened/resaved by Adobe.

use std::{env, fs::File, io::Read};

use aftereffects_file::reader::read_empty_composition;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let [input] = args.as_slice() else {
        return Err("usage: read_empty INPUT".into());
    };
    let mut bytes = Vec::new();
    // Match the framing limit, plus one byte to detect an oversized input.
    File::open(input)?
        .take(256 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    let comp = read_empty_composition(&bytes)?;
    println!(
        "{}: {}x{}, {} seconds, 24fps, id={}",
        comp.name, comp.width, comp.height, comp.duration_secs, comp.id
    );
    Ok(())
}
