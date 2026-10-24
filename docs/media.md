# Native media

`media::Frame` stores a square RGBA image and its duration in milliseconds.
The allowed dimensions are 16, 32, and 64 pixels. `Frame::from_image` and
`Frame::from_bytes` resize with nearest-neighbor sampling. Non-square sources
stretch to the selected square resolution.

The native Bluetooth encoder supports 16 and 32 pixels. It rejects 64 pixel
native encoding because the available protocol evidence does not specify its
frame flag, palette fields, or model behavior. Loading, resizing, and rasterizing
64 pixel images still work. Pixoo-64 HTTP commands are a different protocol.

`Frame::encode_native` writes one AA frame and retains its duration.
`Frame::static_packet` writes command `0x44` with the observed fixed prefix
`00 0a 0a 04` and a zero-duration AA frame.

`Animation::new` requires at least one frame and one common resolution.
`Animation::encode_native` emits all AA frames. It prepends the observed 05 and
06 containers for 32 pixel animations. A one-frame animation has duration zero,
matching the Go fixture behavior. Multiple frames retain their individual
durations, including zero.

`Animation::packets` splits the native stream into command `0x49` packets with
200 data bytes each, except the final packet. Select `MediaProfile::Standard`
for the official u16 total size and u8 chunk index. Select
`MediaProfile::PixooMax` for u32 total size and u16 chunk index. Counter widths
depend on the device, including when a Pixoo Max displays 16 pixel content.
Both variants reject an unrepresentable total or index before returning packets.
`chunk_native` permits transport of an externally encoded stream without
claiming that its content has been validated.

The encoder discovers RGB colors in their first row-major occurrence. Pixels
with alpha at most 32 become black. Other pixels retain their straight RGB
channels. Palette indices use the minimum bit count in least-significant-bit
order, with one bit even for a one-color palette. A palette count of zero means
the maximum palette for that frame resolution. `Animation::decode_native`
checks every container length, palette, pixel index, and trailing byte. It also
reads the observed 16 pixel palette-reuse flag, appending local colors to the
previous palette. Encoding uses independent palettes.

## GIF loading

`Animation::from_gif` uses the `gif` decoder and composites full logical-screen
images before resizing. Transparent source pixels leave the canvas unchanged.
Disposal `Background` restores the previous frame rectangle to the logical
screen's global background color, or transparent black if no background exists.
Disposal `Previous` restores the canvas saved before drawing the frame. `Keep`
and unspecified disposal retain the displayed canvas.

GIF delays are centiseconds. The loader multiplies them by ten without applying
a minimum delay. Delays above 65535 milliseconds return an error rather than
wrapping or clamping. GIF repetition metadata is not represented by the native
stream. GIFs requiring user input are unsupported. Empty canvases, rectangles
outside the canvas, canvases over 16 million pixels, and animations over 4096
frames return errors.

## Text rendering

`TextBitmap` accepts rasterized monochrome columns from a caller's font renderer.
Bit zero is the top row. The constructor validates the declared height and
limits width to 4096 columns. `static_frame` aligns the bitmap horizontally and
vertically with clipping. `scrolling` moves it left by one column per frame,
including blank first and last frames, with a caller-supplied duration.

This module does not parse TTF or BDF fonts. Font shaping and glyph rasterization
belong to the caller. It does not serialize native command `0x87` word content.
Official page 292 describes the C fields and names
`divoom_image_encode_frame_word`, but omits that function's wire serialization.
Packing the C structure directly or assuming UTF-8 would invent protocol bytes.
The command module accepts an externally encoded word block for that operation.

## Evidence and verification

- [Official command 0x44](official/289.json) delegates its payload to an
  unpublished encoder.
- [Official command 0x49](official/290.json) specifies standard counters and
  200-byte chunks.
- [Official command 0x87](official/292.json) describes text attributes without
  defining the word encoder's output bytes.
- The native palette layout, Pixoo Max counters and flag containers come from
  `vendor/go-divoom/encode.go`. Tests compare actual Go golden fixtures copied
  into `tests/fixtures/go-goldens.json`.
- [The GIF89a specification](https://giflib.sourceforge.net/gifstandard/GIF89a.html)
  defines the logical-screen background and frame disposal methods.

The Rust reference under `.audit/rust-reference` was inspected for behavior.
It has no discoverable license, so its source was not copied. Its treatment of
truncated native input and zero-bit one-color palettes was not adopted.

Run `cargo test --lib media::tests` to check golden bytes, per-frame timing,
palette limits and reuse, GIF disposal and transparency, malformed native
input, model counters, and text alignment and scrolling.
