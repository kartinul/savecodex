//! Embeds the compiled frontend (`frontend/dist/`) into the binary.

use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../../frontend/dist/"]
pub struct Frontend;
