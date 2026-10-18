//! Palette files: GIMP's .gpl, .pal (JASC's text, Microsoft's RIFF, or bare RGB
//! triples), Adobe's .act colour tables and .ase swatch exchange files. All of them
//! are read, and written in the first of their kinds.

use std::{fmt, fs, io, path::Path};

use image::Rgba;

use crate::{MAX_PALETTE_SIZE, Palette};

/// File extensions palettes are read from and written to, lower case.
pub const PALETTE_EXTENSIONS: &[&str] = &["gpl", "pal", "act", "ase"];

#[derive(Debug)]
pub enum PaletteError {
    Io(io::Error),
    /// The file is not a palette of its kind, or a broken one.
    Format(&'static str),
    /// Its extension names no kind of palette file.
    Extension,
}

impl fmt::Display for PaletteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Format(problem) => f.write_str(problem),
            Self::Extension => f.write_str("not a palette file (.gpl, .pal, .act or .ase)"),
        }
    }
}

impl std::error::Error for PaletteError {}

impl From<io::Error> for PaletteError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// The kinds of palette files, by extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Gpl,
    Pal,
    Act,
    Ase,
}

impl Format {
    fn of(path: &Path) -> Result<Self, PaletteError> {
        let extension = path.extension().and_then(|extension| extension.to_str()).unwrap_or_default();
        match extension.to_ascii_lowercase().as_str() {
            "gpl" => Ok(Self::Gpl),
            "pal" => Ok(Self::Pal),
            "act" => Ok(Self::Act),
            "ase" => Ok(Self::Ase),
            _ => Err(PaletteError::Extension),
        }
    }
}

/// Reads a palette file, of the kind its extension names. Its first
/// [`MAX_PALETTE_SIZE`] colours are kept.
pub fn load_palette(path: &Path) -> Result<Palette, PaletteError> {
    let format = Format::of(path)?;
    read_palette(&fs::read(path)?, format)
}

/// Writes the palette to a file, of the kind its extension names.
pub fn save_palette(path: &Path, palette: &Palette) -> Result<(), PaletteError> {
    let format = Format::of(path)?;
    let name = path.file_stem().map(|stem| stem.to_string_lossy()).unwrap_or_default();
    fs::write(path, write_palette(palette, format, &name))?;
    Ok(())
}

fn read_palette(bytes: &[u8], format: Format) -> Result<Palette, PaletteError> {
    let mut palette = match format {
        Format::Gpl => read_gpl(bytes)?,
        Format::Pal if bytes.starts_with(b"RIFF") => read_riff(bytes)?,
        Format::Pal if bytes.starts_with(b"JASC-PAL") => read_jasc(bytes)?,
        Format::Pal if bytes.len() == 768 => read_act(bytes)?,
        Format::Pal => return Err(PaletteError::Format("not a JASC, RIFF or RGB palette")),
        Format::Act => read_act(bytes)?,
        Format::Ase => read_ase(bytes)?,
    };
    if palette.is_empty() {
        return Err(PaletteError::Format("the palette has no colors"));
    }
    palette.resize(palette.len().min(MAX_PALETTE_SIZE));
    Ok(palette)
}

fn write_palette(palette: &Palette, format: Format, name: &str) -> Vec<u8> {
    match format {
        Format::Gpl => write_gpl(palette, name),
        Format::Pal => write_jasc(palette),
        Format::Act => write_act(palette),
        Format::Ase => write_ase(palette),
    }
}

fn text(bytes: &[u8]) -> Result<&str, PaletteError> {
    std::str::from_utf8(bytes).map_err(|_| PaletteError::Format("the palette is not text"))
}

/// The colour of the channels a line starts with, three or, with `alpha`, four.
fn parse_channels(line: &str, alpha: bool) -> Result<Rgba<u8>, PaletteError> {
    let mut numbers = line.split_whitespace().map(str::parse::<u8>);
    let mut next = || match numbers.next() {
        Some(Ok(number)) => Ok(number),
        _ => Err(PaletteError::Format("a color is not three numbers from 0 to 255")),
    };
    Ok(Rgba([next()?, next()?, next()?, if alpha { next()? } else { 255 }]))
}

fn has_alpha(palette: &Palette) -> bool {
    palette.colors.iter().any(|color| color[3] < 255)
}

/// GIMP's palette: a header, then a colour a line as its channels and a name. As
/// Aseprite writes them, a `Channels: RGBA` header gives each colour its alpha.
fn read_gpl(bytes: &[u8]) -> Result<Palette, PaletteError> {
    let mut lines = text(bytes)?.lines();
    if lines.next().map(str::trim) != Some("GIMP Palette") {
        return Err(PaletteError::Format("not a GIMP palette"));
    }
    let mut alpha = false;
    let mut colors = Vec::new();
    for line in lines.map(str::trim) {
        if line.starts_with("Channels:") {
            alpha = line.ends_with("RGBA");
        } else if !(line.is_empty() || line.starts_with('#') || line.starts_with("Name:") || line.starts_with("Columns:")) {
            colors.push(parse_channels(line, alpha)?);
        }
    }
    Ok(Palette::new(colors))
}

fn write_gpl(palette: &Palette, name: &str) -> Vec<u8> {
    let alpha = has_alpha(palette);
    let mut text = format!("GIMP Palette\nName: {name}\nColumns: 16\n");
    if alpha {
        text.push_str("Channels: RGBA\n");
    }
    text.push_str("#\n");
    for &Rgba([red, green, blue, opacity]) in &palette.colors {
        text.push_str(&format!("{red:3} {green:3} {blue:3}"));
        if alpha {
            text.push_str(&format!(" {opacity:3}"));
        }
        text.push_str(&format!("\t#{red:02X}{green:02X}{blue:02X}\n"));
    }
    text.into_bytes()
}

/// JASC's (Paint Shop Pro's) palette: a header, the count, then a colour a line. A
/// fourth number on a line, as Aseprite writes, is its alpha.
fn read_jasc(bytes: &[u8]) -> Result<Palette, PaletteError> {
    let mut lines = text(bytes)?.lines().map(str::trim).filter(|line| !line.is_empty()).skip(2);
    let count: usize = lines
        .next()
        .and_then(|count| count.parse().ok())
        .ok_or(PaletteError::Format("the JASC palette has no count"))?;
    let colors = lines
        .take(count)
        .map(|line| parse_channels(line, line.split_whitespace().count() >= 4))
        .collect::<Result<_, _>>()?;
    Ok(Palette::new(colors))
}

fn write_jasc(palette: &Palette) -> Vec<u8> {
    let alpha = has_alpha(palette);
    let mut text = format!("JASC-PAL\r\n0100\r\n{}\r\n", palette.len());
    for &Rgba([red, green, blue, opacity]) in &palette.colors {
        text.push_str(&format!("{red} {green} {blue}"));
        if alpha {
            text.push_str(&format!(" {opacity}"));
        }
        text.push_str("\r\n");
    }
    text.into_bytes()
}

/// Microsoft's RIFF palette: a `data` chunk of a version, a count, and four bytes a
/// colour, the last of them flags.
fn read_riff(bytes: &[u8]) -> Result<Palette, PaletteError> {
    const BROKEN: PaletteError = PaletteError::Format("the RIFF palette is broken");
    if bytes.get(8..12) != Some(b"PAL ") {
        return Err(PaletteError::Format("not a RIFF palette"));
    }
    let mut at = 12;
    while let Some(header) = bytes.get(at..at + 8) {
        let size = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let chunk = bytes.get(at + 8..at + 8 + size).ok_or(BROKEN)?;
        if &header[..4] == b"data" {
            let count = u16::from_le_bytes(chunk.get(2..4).ok_or(BROKEN)?.try_into().unwrap()) as usize;
            let entries = chunk.get(4..4 + count * 4).ok_or(BROKEN)?;
            let colors = entries.as_chunks::<4>().0.iter().map(|&[red, green, blue, _]| Rgba([red, green, blue, 255])).collect();
            return Ok(Palette::new(colors));
        }
        // Chunks are padded to an even size.
        at += 8 + size + size % 2;
    }
    Err(BROKEN)
}

/// Adobe's colour table: 256 colours as RGB triples, then, in newer ones, how many
/// of them are used and which is transparent, 0xFFFF for none, both big-endian.
fn read_act(bytes: &[u8]) -> Result<Palette, PaletteError> {
    let table = bytes.get(..768).ok_or(PaletteError::Format("the color table is too short"))?;
    let (count, transparent) = match bytes.get(768..772) {
        Some(end) => (u16::from_be_bytes([end[0], end[1]]) as usize, u16::from_be_bytes([end[2], end[3]]) as usize),
        None => (256, 0xFFFF),
    };
    // Some write 0 for all 256.
    let count = if count == 0 { 256 } else { count.min(256) };
    let colors = table.as_chunks::<3>().0.iter().take(count).map(|&[red, green, blue]| Rgba([red, green, blue, 255])).collect();
    let transparent = Some(transparent).filter(|&transparent| transparent < count);
    Ok(Palette { colors, transparent })
}

fn write_act(palette: &Palette) -> Vec<u8> {
    let mut bytes = vec![0; 768];
    for (rgb, color) in bytes.as_chunks_mut::<3>().0.iter_mut().zip(&palette.colors) {
        rgb.copy_from_slice(&color.0[..3]);
    }
    bytes.extend((palette.len() as u16).to_be_bytes());
    bytes.extend(palette.transparent.map_or(0xFFFF, |transparent| transparent as u16).to_be_bytes());
    bytes
}

/// Adobe's swatch exchange file: blocks of colours, which groups may gather, each with
/// a name and its channels as floats, all big-endian. RGB, grey, CMYK and Lab colours
/// are read, CMYK as the simplest mix of inks.
fn read_ase(bytes: &[u8]) -> Result<Palette, PaletteError> {
    const BROKEN: PaletteError = PaletteError::Format("the swatch file is broken");
    if !bytes.starts_with(b"ASEF") {
        return Err(PaletteError::Format("not an Adobe swatch exchange file"));
    }
    let u16_at = |at: usize| bytes.get(at..at + 2).map(|b| u16::from_be_bytes([b[0], b[1]])).ok_or(BROKEN);
    let u32_at = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap())).ok_or(BROKEN);
    let f32_at = |at: usize| u32_at(at).map(f32::from_bits);
    let blocks = u32_at(8)?;
    let mut at = 12;
    let mut colors = Vec::new();
    for _ in 0..blocks {
        let (kind, length) = (u16_at(at)?, u32_at(at + 2)? as usize);
        let body = at + 6;
        if kind == 0x0001 {
            let model_at = body + 2 + u16_at(body)? as usize * 2;
            let model = bytes.get(model_at..model_at + 4).ok_or(BROKEN)?;
            let value = |index: usize| f32_at(model_at + 4 + index * 4);
            let rgb = match model {
                b"RGB " => [value(0)?, value(1)?, value(2)?],
                b"Gray" => [value(0)?; 3],
                b"CMYK" => {
                    let black = 1.0 - value(3)?;
                    [(1.0 - value(0)?) * black, (1.0 - value(1)?) * black, (1.0 - value(2)?) * black]
                }
                b"LAB " => lab_to_srgb(value(0)? * 100.0, value(1)?, value(2)?),
                _ => return Err(PaletteError::Format("the swatch file has a color of an unknown kind")),
            };
            let [red, green, blue] = rgb.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8);
            colors.push(Rgba([red, green, blue, 255]));
        }
        at = body + length;
    }
    Ok(Palette::new(colors))
}

/// A CIE Lab colour, of the D50 white swatch files use, as sRGB channels from 0 to 1.
fn lab_to_srgb(lightness: f32, a: f32, b: f32) -> [f32; 3] {
    const WHITE: [f32; 3] = [0.96422, 1.0, 0.82521];
    // XYZ, adapted from D50 to D65 by Bradford's method, to linear sRGB.
    const TO_RGB: [[f32; 3]; 3] = [
        [3.133_856, -1.616_866_7, -0.490_614_6],
        [-0.9787684, 1.9161415, 0.0334540],
        [0.0719453, -0.2289914, 1.4052427],
    ];
    let y = (lightness + 16.0) / 116.0;
    let inverse = |t: f32| if t > 6.0 / 29.0 { t * t * t } else { 3.0 * (6.0f32 / 29.0).powi(2) * (t - 4.0 / 29.0) };
    let xyz = [inverse(y + a / 500.0) * WHITE[0], inverse(y) * WHITE[1], inverse(y - b / 200.0) * WHITE[2]];
    TO_RGB.map(|row| {
        let linear: f32 = row.iter().zip(xyz).map(|(factor, part)| factor * part).sum();
        if linear <= 0.0031308 { 12.92 * linear } else { 1.055 * linear.powf(1.0 / 2.4) - 0.055 }
    })
}

/// Each colour as an RGB swatch named after its code, in no group.
fn write_ase(palette: &Palette) -> Vec<u8> {
    let mut bytes = b"ASEF".to_vec();
    bytes.extend(1u16.to_be_bytes());
    bytes.extend(0u16.to_be_bytes());
    bytes.extend((palette.len() as u32).to_be_bytes());
    for &Rgba([red, green, blue, _]) in &palette.colors {
        let name: Vec<u16> = format!("#{red:02X}{green:02X}{blue:02X}").encode_utf16().chain([0]).collect();
        let mut body = (name.len() as u16).to_be_bytes().to_vec();
        body.extend(name.iter().flat_map(|unit| unit.to_be_bytes()));
        body.extend(b"RGB ");
        for channel in [red, green, blue] {
            body.extend((channel as f32 / 255.0).to_be_bytes());
        }
        // A global colour.
        body.extend(0u16.to_be_bytes());
        bytes.extend(0x0001u16.to_be_bytes());
        bytes.extend((body.len() as u32).to_be_bytes());
        bytes.extend(body);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette {
        let colors = vec![Rgba([0, 0, 0, 255]), Rgba([255, 128, 1, 255]), Rgba([20, 40, 60, 255])];
        Palette { colors, transparent: None }
    }

    #[test]
    fn each_format_reads_back_what_it_writes() {
        for format in [Format::Gpl, Format::Pal, Format::Act, Format::Ase] {
            let read = read_palette(&write_palette(&palette(), format, "Test"), format).unwrap();
            assert_eq!(read, palette(), "{format:?}");
        }
    }

    #[test]
    fn color_tables_keep_the_transparent_index() {
        let palette = Palette { transparent: Some(1), ..palette() };
        assert_eq!(read_palette(&write_act(&palette), Format::Act).unwrap(), palette);
    }

    #[test]
    fn text_formats_keep_alpha_when_there_is_some() {
        let mut palette = palette();
        palette.colors[2][3] = 100;
        for format in [Format::Gpl, Format::Pal] {
            let written = write_palette(&palette, format, "Test");
            assert_eq!(read_palette(&written, format).unwrap(), palette, "{format:?}");
        }
        assert!(!String::from_utf8(write_gpl(&self::palette(), "Test")).unwrap().contains("Channels"));
    }

    #[test]
    fn gimp_palettes_skip_their_headers_and_comments() {
        let file = "GIMP Palette\nName: Two\nColumns: 4\n# A comment\n\n  0   0   0\tBlack\n255 255 255 White\n";
        let palette = read_palette(file.as_bytes(), Format::Gpl).unwrap();
        assert_eq!(palette.colors, [Rgba([0, 0, 0, 255]), Rgba([255, 255, 255, 255])]);
        assert!(read_palette(b"GIMP Palette\n1 2\n", Format::Gpl).is_err());
        assert!(read_palette(b"Not a palette\n", Format::Gpl).is_err());
    }

    #[test]
    fn riff_and_bare_pal_files_are_read() {
        let mut riff = b"RIFF\0\0\0\0PAL data\x0c\0\0\0\0\x03\x02\0".to_vec();
        riff.extend([255, 0, 0, 0, 0, 0, 255, 0]);
        let palette = read_palette(&riff, Format::Pal).unwrap();
        assert_eq!(palette.colors, [Rgba([255, 0, 0, 255]), Rgba([0, 0, 255, 255])]);
        let bare = vec![7; 768];
        assert_eq!(read_palette(&bare, Format::Pal).unwrap().colors, [Rgba([7, 7, 7, 255]); 256]);
    }

    #[test]
    fn lab_swatches_are_converted_to_srgb() {
        let white = lab_to_srgb(100.0, 0.0, 0.0).map(|channel| (channel * 255.0).round() as u8);
        assert_eq!(white, [255, 255, 255]);
        let red = lab_to_srgb(54.29, 80.81, 69.89).map(|channel| (channel * 255.0).round() as i32);
        assert!(red[0] >= 250 && red[1] <= 5 && red[2] <= 5, "{red:?}");
    }

    #[test]
    fn unknown_extensions_are_refused() {
        assert!(matches!(Format::of(Path::new("colors.txt")), Err(PaletteError::Extension)));
        assert_eq!(Format::of(Path::new("COLORS.GPL")).unwrap(), Format::Gpl);
    }
}
