use lopdf::{Document, Object, Stream, dictionary};

use super::*;
use model::jpeg::markers::{APP14, EOI, SOI, write_segment};

fn picture(extra: lopdf::Dictionary, samples: Vec<u8>) -> Stream {
    let mut dict = dictionary! {
        "Type" => "XObject", "Subtype" => "Image", "Width" => 16, "Height" => 16,
        "BitsPerComponent" => 8,
    };
    dict.extend(&extra);
    Stream::new(dict, samples)
}

fn indexed(doc: &mut Document, base: &str, palette: Vec<u8>) -> Object {
    let space: Vec<Object> = vec![
        "Indexed".into(),
        base.into(),
        (palette.len() as i64 / if base == "DeviceGray" { 1 } else { 3 } - 1).into(),
        Object::string_literal(palette),
    ];
    doc.add_object(Object::Array(space)).into()
}

#[test]
fn a_picture_is_read_as_plain_indexed_or_cmyk_and_nothing_stranger() {
    let mut doc = Document::with_version("1.7");
    let read = |doc: &Document, s: &Stream| colour(doc, s);

    let rgb = picture(dictionary! { "ColorSpace" => "DeviceRGB" }, vec![]);
    assert!(matches!(
        read(&doc, &rgb).unwrap().samples,
        Samples::Plain(3)
    ));
    let cmyk = picture(dictionary! { "ColorSpace" => "DeviceCMYK" }, vec![]);
    assert!(matches!(read(&doc, &cmyk).unwrap().samples, Samples::Cmyk));

    let profile = doc.add_object(Stream::new(dictionary! { "N" => 1 }, vec![]));
    let icc = picture(
        dictionary! { "ColorSpace" => vec![Object::from("ICCBased"), profile.into()] },
        vec![],
    );
    assert!(matches!(
        read(&doc, &icc).unwrap().samples,
        Samples::Plain(1)
    ));

    let space = indexed(&mut doc, "DeviceRGB", vec![255, 0, 0, 0, 0, 255]);
    let paletted = picture(dictionary! { "ColorSpace" => space.clone() }, vec![]);
    let got = read(&doc, &paletted).unwrap();
    assert!(
        matches!(&got.samples, Samples::Indexed { base: 3, palette } if palette.len() == 6),
        "a palette comes through, and the picture will be in its base space"
    );
    assert_eq!(got.space, Object::from("DeviceRGB"));

    let gray = indexed(&mut doc, "DeviceGray", vec![0, 255]);
    let gray = picture(dictionary! { "ColorSpace" => gray }, vec![]);
    assert!(matches!(
        read(&doc, &gray).unwrap().samples,
        Samples::Indexed { base: 1, .. }
    ));

    let refused = |extra: lopdf::Dictionary| {
        let mut d = dictionary! { "ColorSpace" => space.clone() };
        d.extend(&extra);
        read(&doc, &picture(d, vec![])).is_none()
    };
    assert!(refused(dictionary! { "BitsPerComponent" => 4 }));
    assert!(refused(
        dictionary! { "Mask" => vec![Object::Integer(3), 3.into()] }
    ));
    assert!(refused(
        dictionary! { "Decode" => vec![Object::Integer(255), 0.into()] }
    ));
    assert!(refused(dictionary! { "ImageMask" => true }));
    let on_cmyk = indexed(&mut Document::with_version("1.7"), "DeviceCMYK", vec![0; 8]);
    let base_cmyk = picture(dictionary! { "ColorSpace" => on_cmyk }, vec![]);
    assert!(
        read(&Document::with_version("1.7"), &base_cmyk).is_none(),
        "a palette of CMYK is left alone"
    );
    let flipped = picture(
        dictionary! { "ColorSpace" => "DeviceRGB", "Decode" => vec![Object::Integer(1), 0.into()] },
        vec![],
    );
    assert!(
        read(&doc, &flipped).is_none(),
        "an RGB picture may not be inverted"
    );
}

#[test]
fn an_indexed_picture_goes_to_imagemagick_as_the_colours_it_shows() {
    let mut doc = Document::with_version("1.7");
    let space = indexed(&mut doc, "DeviceRGB", vec![255, 0, 0, 0, 0, 255]);
    let mut p = picture(dictionary! { "ColorSpace" => space }, [0, 1].repeat(128));
    let colour = colour(&doc, &p).unwrap();
    let (pnm, ext) = source(&p, &colour, (16, 16)).unwrap();
    assert_eq!(ext, "ppm");
    assert!(pnm.starts_with(b"P6\n16 16\n255\n"));
    assert_eq!(
        &pnm[13..19],
        [255, 0, 0, 0, 0, 255],
        "indices are now colours"
    );
    assert_eq!(pnm.len(), 13 + 16 * 16 * 3);

    p.compress().unwrap();
    assert!(
        source(&p, &colour, (16, 16)).is_some(),
        "deflated reads the same"
    );
    assert!(
        source(&p, &colour, (17, 16)).is_none(),
        "samples that don't fit the size are not guessed at"
    );
}

#[test]
fn cmyk_is_only_taken_as_the_adobe_way_writes_it() {
    let doc = Document::with_version("1.7");
    let mut jpeg = vec![0xFF, SOI];
    write_segment(&mut jpeg, APP14, b"Adobe\0\x64\0\0\0\0\x02");
    jpeg.extend_from_slice(&[0xFF, EOI]);
    let inverted = vec![
        Object::Integer(1),
        0.into(),
        1.into(),
        0.into(),
        1.into(),
        0.into(),
        1.into(),
        0.into(),
    ];
    let dct = |decode: Option<&Vec<Object>>, filter: &str| {
        let mut d = dictionary! { "ColorSpace" => "DeviceCMYK", "Filter" => filter };
        if let Some(decode) = decode {
            d.set("Decode", decode.clone());
        }
        picture(d, jpeg.clone())
    };
    let ready = |s: &Stream| {
        let colour = colour(&doc, s).unwrap();
        source(s, &colour, (16, 16))
    };
    assert_eq!(ready(&dct(Some(&inverted), "DCTDecode")).unwrap().1, "jpg");
    assert!(
        ready(&dct(None, "DCTDecode")).is_none(),
        "nothing undoes the inversion"
    );
    assert!(
        ready(&dct(Some(&inverted), "FlateDecode")).is_none(),
        "raw CMYK is not ImageMagick's to read"
    );
}
