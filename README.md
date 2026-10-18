# NukePaint

A desktop image editor with pixel and palette management and advanced
transformations, written in Rust.

NukePaint is in early development. See [CHANGELOG.md](CHANGELOG.md) for what it
can do so far.

## Features

- Layers with blend modes, blended on the GPU, and images of any size.
- Selection tools (rectangle, ellipse, lasso, polygon, magic wand) whose pixels can be moved, scaled, rotated, sheared and put in perspective.
- Drawing tools: paintbrush, pencil, eraser, spray, paint bucket, gradient, lines, curves and editable shapes.
- A palette for every document, with a palette editor, presets, and GIMP, JASC/RIFF, ACT and ASE palette files.
- Color adjustments with live preview, and around seventy filters.
- Tiled view with wrap-around painting, for seamless textures.
- Opens PNG, JPEG, BMP, WebP, TGA, SVG and OpenRaster files, and saves all of them but SVG.

## Building

You need a recent stable [Rust](https://rustup.rs) toolchain (edition 2024).

```sh
cargo run -p nukepaint --release
```

## Project layout

- `core/`: the document model and pixel operations, with no UI.
- `app/`: the application, built on eframe, egui and wgpu.
- `icons/`: the toolbar and cursor icons, as SVG.
- `icon.svg`: the app icon.

During development, [bacon](https://dystroy.org/bacon/) can run from the root:
`bacon` runs clippy as files change, and `bacon app` restarts the app on each change.

## AI assistance

NukePaint is developed with the help of AI coding tools. All code is reviewed,
tested and directed by the author.

## License

Copyright (C) 2026 Daniel Orchanian (do-sieg)

NukePaint is free software: you can redistribute it and/or modify it under the
terms of the GNU General Public License as published by the Free Software
Foundation, either version 3 of the License, or (at your option) any later
version. See [LICENSE](LICENSE) for the full text.

The NukePaint name and logo are not covered by this license. Forks and
modified versions must use a different name.
