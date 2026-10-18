# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

First version of NukePaint, a desktop image editor written in Rust with egui
and wgpu.

### Added

#### Files

- Open PNG, JPEG, BMP, WebP, TGA, SVG and OpenRaster (.ora) images.
- Save to PNG, JPEG, WebP, BMP, TGA and OpenRaster, with Save and Save As.
- Unsaved changes marker, and prompts before closing or quitting with unsaved changes.
- Recent files, listed on the start screen and in File > Open Recent.
- Image strip with thumbnails to switch between open images, reordered by dragging.
- Images of any size, shown with tiled GPU textures.

#### Editing

- Undo and redo, with a History panel.
- Cut, Copy and Paste, using the system clipboard.
- Crop, Resize Image and Canvas Size.
- Flip, Rotate 90° and Rotate 180°, for the whole image or a layer, and aware of the selection.

#### Layers

- Layers panel, with layers blended on the GPU.
- Blend modes, including Reflect, Glow, Negation, Xor and Grain Merge.
- Show Only Active Layer option.

#### Selection

- Rectangle, Ellipse, Lasso, Polygon and Magic Wand selection tools.
- Selection modes: replace, add, subtract and intersect.
- Select All, Deselect and Invert Selection.
- Move, resize and rotate the selection or its pixels, with a movable pivot, nudging and a choice of resampling.
- Shear and Perspective tools.
- Drawing tools stay inside the selection.

#### Drawing

- Paintbrush, Pencil, Eraser and Spray tools, with tip size and antialiasing.
- Paint Bucket and Gradient tools.
- Color Picker tool, also reachable from drawing tools with a right click.
- Line, Curve, Rectangle, Ellipse, Contour and Polygon tools. Shapes stay editable until committed, with fill and overlap options.
- Blur, Sharpen and Jumble brushes.

#### Colors and palettes

- Colors panel with primary and secondary colors.
- A palette for every document, shown in the Palette panel and rebuilt from the image's colors on open.
- Palette editor, with preset palettes, and palette files (GIMP .gpl, JASC and RIFF .pal, .act, .ase).
- Colors menu with a live preview dialog:
  - Auto-Level, Equalize and White Balance.
  - Brightness / Contrast, Exposure, Highlights / Shadows, Levels and Curves, with a curve editor.
  - Hue / Saturation, Color Balance, Temperature / Tint and Colorize.
  - Replace Color, Gradient Map and Palette Map (maps the image's lightness onto the document palette).
  - Black and White, Sepia and Threshold.
  - Posterize and Dither.
  - Invert Colors, Invert Alpha and Invert.
  - Color to Alpha.
- Repeat Last Adjustment and Repeat Last Filter.

#### Filters

- Artistic, Blurs, Decor, Distort, Enhance, Light, Map, Noise, Photo, Render and Stylize menus, with niche filters in More submenus.
- Rotate / Zoom and 3D Transform effects.

#### View and interface

- Zoom and Pan tools, View menu, shortcuts and a zoom picker.
- Navigator panel.
- Tiled view of the image or selection, with wrap-around painting.
- Grid and checkerboard settings.
- Dock with tools, panels and sections that can be moved, folded and hidden, placed at the side, top or bottom.
- Dialogs that can be moved and stay open on outside clicks.
- Settings and window layout remembered between sessions.
