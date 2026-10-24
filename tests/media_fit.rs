#![cfg(feature = "media")]
use divoom_bluetooth::media::{Animation, Dimension, FitMode, Frame};
use image::{DynamicImage, Rgba, RgbaImage};

#[test]
fn fitting_preserves_aspect_ratio_and_legacy_stretch() {
    let source = DynamicImage::ImageRgba8(RgbaImage::from_pixel(4, 2, Rgba([255, 0, 0, 255])));
    let contain =
        Frame::from_image_fitted(&source, Dimension::ThirtyTwo, 20, FitMode::Contain).unwrap();
    assert_eq!(contain.image().get_pixel(0, 0).0, [0, 0, 0, 255]);
    assert_eq!(contain.image().get_pixel(0, 8).0, [255, 0, 0, 255]);
    assert_eq!(contain.duration_ms(), 20);
    let stretch = Frame::from_image(&source, Dimension::ThirtyTwo, 20).unwrap();
    assert_eq!(stretch.image().get_pixel(0, 0).0, [255, 0, 0, 255]);
    let cover =
        Frame::from_image_fitted(&source, Dimension::ThirtyTwo, 20, FitMode::Cover).unwrap();
    assert_eq!(cover.image(), stretch.image());
}

#[test]
fn cover_samples_centered_pixels_at_extreme_and_odd_aspect_ratios() {
    for (width, height) in [(65535, 1), (1, 65535)] {
        let mut source = RgbaImage::from_pixel(width, height, Rgba([255, 0, 0, 255]));
        source.put_pixel(width / 2, height / 2, Rgba([0, 0, 255, 255]));
        let frame = Frame::from_image_fitted(
            &DynamicImage::ImageRgba8(source),
            Dimension::ThirtyTwo,
            50,
            FitMode::Cover,
        )
        .unwrap();
        assert!(frame
            .image()
            .pixels()
            .all(|pixel| pixel.0 == [0, 0, 255, 255]));
        assert_eq!(frame.duration_ms(), 50);
    }
    let source = RgbaImage::from_fn(3, 2, |x, _| {
        Rgba(match x {
            0 => [255, 0, 0, 255],
            1 => [0, 255, 0, 255],
            _ => [0, 0, 255, 255],
        })
    });
    let frame = Frame::from_image_fitted(
        &DynamicImage::ImageRgba8(source),
        Dimension::ThirtyTwo,
        0,
        FitMode::Cover,
    )
    .unwrap();
    assert_eq!(frame.image().get_pixel(7, 16).0, [255, 0, 0, 255]);
    assert_eq!(frame.image().get_pixel(8, 16).0, [0, 255, 0, 255]);
    assert_eq!(frame.image().get_pixel(23, 16).0, [0, 255, 0, 255]);
    assert_eq!(frame.image().get_pixel(24, 16).0, [0, 0, 255, 255]);
}

#[test]
fn gif_fit_happens_after_background_disposal() {
    let mut bytes = Vec::new();
    {
        let mut encoder =
            gif::Encoder::new(&mut bytes, 2, 1, &[0, 0, 0, 255, 0, 0, 0, 0, 255]).unwrap();
        encoder
            .write_frame(&gif::Frame {
                width: 2,
                height: 1,
                delay: 2,
                buffer: vec![1, 1].into(),
                ..Default::default()
            })
            .unwrap();
        encoder
            .write_frame(&gif::Frame {
                width: 1,
                height: 1,
                delay: 3,
                dispose: gif::DisposalMethod::Background,
                buffer: vec![2].into(),
                ..Default::default()
            })
            .unwrap();
        encoder
            .write_frame(&gif::Frame {
                left: 1,
                width: 1,
                height: 1,
                delay: 4,
                buffer: vec![2].into(),
                ..Default::default()
            })
            .unwrap();
    }
    let animation =
        Animation::from_gif_fitted(bytes.as_slice(), Dimension::ThirtyTwo, FitMode::Contain)
            .unwrap();
    assert_eq!(
        animation
            .frames()
            .iter()
            .map(Frame::duration_ms)
            .collect::<Vec<_>>(),
        vec![20, 30, 40]
    );
    assert_eq!(
        animation.frames()[1].image().get_pixel(0, 8).0,
        [0, 0, 255, 255]
    );
    assert_eq!(
        animation.frames()[2].image().get_pixel(0, 8).0,
        [0, 0, 0, 255]
    );
    assert_eq!(
        animation.frames()[2].image().get_pixel(31, 8).0,
        [0, 0, 255, 255]
    );
    assert_eq!(
        animation.frames()[2].image().get_pixel(0, 0).0,
        [0, 0, 0, 255]
    );
}
