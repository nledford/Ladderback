//! Embeds the catalogs and generates the message IDs. Stubbed: writes empty
//! tables, so the crate compiles before the real generator exists.

use std::path::PathBuf;
use std::{env, fs};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(env::var("OUT_DIR")?);
    fs::write(
        out.join("catalog.rs"),
        "pub(crate) const CATALOGS: &[Catalog] = &[];\n",
    )?;
    fs::write(out.join("msg.rs"), "pub const ALL: &[MessageId] = &[];\n")?;
    Ok(())
}
