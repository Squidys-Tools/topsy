//! The background model: BiRefNet-lite's own weights (MIT), at a fixed revision of
//! ZhengPeng7/BiRefNet_lite, fetched once (see `transform::fetch`).

use crate::transform::fetch::Pinned;

pub const MODEL: Pinned = Pinned {
    label: "the background model",
    host: "huggingface.co",
    path: "/ZhengPeng7/BiRefNet_lite/resolve/aa62cd87eafb9cc43056d08ef3615a14628b831d/model.safetensors",
    size: 177_634_392,
    sha256: "4417d89795250e698c3cb0ae8df15743810065f646f48a694fdfa7ca052d0815",
    file: r"models\birefnet-lite-aa62cd8.safetensors",
};
