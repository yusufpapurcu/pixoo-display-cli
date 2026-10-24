//! Build reviewable native bytes without opening a Bluetooth connection.
use divoom_bluetooth::media::{Alignment, Animation, Dimension, Frame, MediaProfile, TextBitmap};
use divoom_bluetooth::Result;
use image::{Rgba, RgbaImage};

fn main() -> Result<()> {
    let red = Frame::new(RgbaImage::from_pixel(16, 16, Rgba([255, 0, 0, 255])), 100)?;
    let blue = Frame::new(RgbaImage::from_pixel(16, 16, Rgba([0, 0, 255, 255])), 250)?;
    let still = red.static_packet()?;
    assert_eq!(still.opcode(), 0x44);

    let animation = Animation::new(vec![red, blue])?;
    let native = animation.encode_native()?;
    assert_eq!(Animation::decode_native(&native)?, animation);
    let packets = animation.packets(MediaProfile::PixooMax)?;
    assert_eq!(packets[0].opcode(), 0x49);

    let glyph = TextBitmap::new(vec![0b111110, 0b001001, 0b001001, 0b111110], 6)?;
    let scrolling = glyph.scrolling(Dimension::Sixteen, Alignment::Center, [255; 3], [0; 3], 60)?;
    assert_eq!(scrolling.frames().len(), 21);
    Ok(())
}
