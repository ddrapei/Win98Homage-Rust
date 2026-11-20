//! Photos off the disk, small: a file decoded by WIC, the Windows Imaging
//! Component that comes with Windows, and brought down to a few thousand
//! pixels, cut to fill the shape asked for and stood the right way up.
//!
//! Only the codecs WIC has of its own are used -- JPEG, PNG, BMP, GIF and
//! TIFF (`FORMATS`).  HEIC, WebP and camera RAW files are decoded on Windows
//! only by extensions from the Microsoft Store, which a machine may or may not
//! have, so they are left alone.
//!
//! The scaler is handed the decoder's frame itself where it can be: a JPEG is
//! then decoded at an eighth, a quarter or half of its size before it is
//! scaled the rest of the way, which is what makes a photo of many megapixels
//! quick to bring down.  WIC is COM; the methods needed are called through
//! their vtable slots, which wincodec.h fixes.

use core::ffi::c_void;
use core::ptr::{null, null_mut};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use crate::sys::{self, release, slot, Com, GUID, HRESULT};

/// The extensions of the files WIC decodes with what Windows itself has.
pub const FORMATS: [&str; 8] = ["jpg", "jpeg", "jpe", "png", "bmp", "gif", "tif", "tiff"];

/// Whether a file is one of `FORMATS`, by its extension.
pub fn is_photo(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| FORMATS.iter().any(|f| f.eq_ignore_ascii_case(e)))
}

const CLSID_WIC_IMAGING_FACTORY: GUID =
    GUID { a: 0xcacaf262, b: 0x9370, c: 0x4615, d: [0xa1, 0x3b, 0x9f, 0x55, 0x39, 0xda, 0x4c, 0x0a] };
const IID_WIC_IMAGING_FACTORY: GUID =
    GUID { a: 0xec5ec8a9, b: 0xc395, c: 0x4314, d: [0x9c, 0x77, 0x54, 0xd7, 0xa9, 0x35, 0xff, 0x70] };
/// GUID_WICPixelFormat32bppPBGRA: premultiplied BGRA, a u32 of 0xAARRGGBB.
const PIXEL_PBGRA: GUID = GUID { a: 0x6fddc324, b: 0x4e03, c: 0x4bfe, d: [0xb1, 0x85, 0x3d, 0x77, 0x76, 0x8d, 0xc9, 0x10] };

const GENERIC_READ: u32 = 0x8000_0000;
const WIC_DECODE_METADATA_CACHE_ON_DEMAND: u32 = 0;
const WIC_DITHER_NONE: u32 = 0;
const WIC_PALETTE_CUSTOM: u32 = 0;
/// Each pixel the average of what it covers: the right way to bring a photo
/// down a long way.
const WIC_INTERPOLATION_FANT: u32 = 3;
const VT_UI2: u16 = 18;

// vtable slots, after IUnknown's three
const FACTORY_CREATE_DECODER_FROM_FILENAME: usize = 3;
const FACTORY_CREATE_FORMAT_CONVERTER: usize = 10;
const FACTORY_CREATE_BITMAP_SCALER: usize = 11;
const DECODER_GET_FRAME: usize = 13;
const SOURCE_GET_SIZE: usize = 3;
const SOURCE_COPY_PIXELS: usize = 7;
const FRAME_GET_METADATA_QUERY_READER: usize = 8;
const CONVERTER_INITIALIZE: usize = 8;
const SCALER_INITIALIZE: usize = 8;
const READER_GET_METADATA_BY_NAME: usize = 5;

/// A COM object, released when it goes.
struct Obj(Com);

impl Drop for Obj {
    fn drop(&mut self) {
        unsafe { release(self.0) };
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct WicRect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

/// What a metadata query answers with: a type, and room for the largest of
/// the values (16 bytes on 64-bit Windows, 8 on 32-bit).
#[repr(C)]
#[derive(Default)]
struct PropVariant {
    vt: u16,
    reserved: [u16; 3],
    value: [usize; 2],
}

/// How large to scale a `sw` x `sh` picture for it to cover `tw` x `th`, and
/// the part of it, in the middle, that is then cut out: the scaled width and
/// height, and that part as x, y, w, h.  The picture keeps its proportions;
/// whatever sticks out of the shape on one side or the other is cut off.
pub fn cover(sw: u32, sh: u32, tw: u32, th: u32) -> (u32, u32, [u32; 4]) {
    let (sw, sh) = (sw.max(1) as f64, sh.max(1) as f64);
    let scale = (tw as f64 / sw).max(th as f64 / sh);
    let w = ((sw * scale).round() as u32).max(tw);
    let h = ((sh * scale).round() as u32).max(th);
    (w, h, [(w - tw) / 2, (h - th) / 2, tw, th])
}

/// A picture stored `w` x `h` as the camera held it, turned and flipped the
/// way its Exif orientation (1-8) says it is to be seen.  5 to 8 stand it on
/// its side, so what comes back is `h` x `w`.
pub fn orient(px: &[u32], w: usize, h: usize, orientation: u16) -> Vec<u32> {
    let turned = (5..=8).contains(&orientation);
    let out_w = if turned { h } else { w };
    let mut out = vec![0u32; px.len()];
    for y in 0..h {
        for x in 0..w {
            // where the stored pixel at (x, y) is seen
            let (ox, oy) = match orientation {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (h - 1 - y, x),
                7 => (h - 1 - y, w - 1 - x),
                8 => (y, w - 1 - x),
                _ => (x, y),
            };
            out[oy * out_w + ox] = px[y * w + x];
        }
    }
    out
}

/// A premultiplied pixel laid on an opaque `bg`, as 0xFFRRGGBB.
fn over(px: u32, bg: u32) -> u32 {
    let a = px >> 24;
    if a == 255 {
        return px;
    }
    let mix = |shift: u32| {
        let (c, b) = ((px >> shift) & 0xFF, (bg >> shift) & 0xFF);
        (c + (b * (255 - a) + 127) / 255).min(255) << shift
    };
    0xFF00_0000 | mix(16) | mix(8) | mix(0)
}

/// The photo in `path` as `w` x `h` opaque pixels (0xFFRRGGBB, row by row):
/// scaled to cover that shape, the middle of it cut out, and turned the way
/// it is to be seen.  What is transparent in it is laid on `bg`.  None if it
/// cannot be read or decoded.
pub fn load(path: &Path, w: u32, h: u32, bg: u32) -> Option<Vec<u32>> {
    if w == 0 || h == 0 || !sys::com_init() {
        return None;
    }
    unsafe {
        let factory = Obj(sys::co_create(&CLSID_WIC_IMAGING_FACTORY, &IID_WIC_IMAGING_FACTORY)?);
        let decoder = {
            type Create = unsafe extern "system" fn(Com, *const u16, *const GUID, u32, u32, *mut Com) -> HRESULT;
            let create: Create = slot(factory.0, FACTORY_CREATE_DECODER_FROM_FILENAME);
            let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let mut out: Com = null_mut();
            if create(factory.0, name.as_ptr(), null(), GENERIC_READ, WIC_DECODE_METADATA_CACHE_ON_DEMAND, &mut out) < 0 || out.is_null() {
                return None;
            }
            Obj(out)
        };
        let frame = {
            let get: unsafe extern "system" fn(Com, u32, *mut Com) -> HRESULT = slot(decoder.0, DECODER_GET_FRAME);
            let mut out: Com = null_mut();
            if get(decoder.0, 0, &mut out) < 0 || out.is_null() {
                return None;
            }
            Obj(out)
        };
        let (mut sw, mut sh) = (0u32, 0u32);
        let size: unsafe extern "system" fn(Com, *mut u32, *mut u32) -> HRESULT = slot(frame.0, SOURCE_GET_SIZE);
        if size(frame.0, &mut sw, &mut sh) < 0 || sw == 0 || sh == 0 {
            return None;
        }
        // Cut out as it is stored -- on its side, for a photo that is seen
        // turned -- and turned afterwards: the middle stays the middle.
        let orientation = orientation_of(frame.0);
        let (tw, th) = if (5..=8).contains(&orientation) { (h, w) } else { (w, h) };
        let (scaled_w, scaled_h, [x, y, cw, ch]) = cover(sw, sh, tw, th);
        let part = WicRect { x: x as i32, y: y as i32, w: cw as i32, h: ch as i32 };
        // Scaled straight from the frame (quick for a JPEG), or -- for a
        // format the scaler will not take as it is, such as one with a
        // palette -- made 32-bit first.
        let px = pixels(factory.0, frame.0, scaled_w, scaled_h, part, true).or_else(|| pixels(factory.0, frame.0, scaled_w, scaled_h, part, false))?;
        let px: Vec<u32> = px.into_iter().map(|p| over(p, bg | 0xFF00_0000)).collect();
        Some(orient(&px, tw as usize, th as usize, orientation))
    }
}

/// The Exif orientation of a frame, 1 (as it is) to 8; 1 for one that says
/// nothing, as a PNG or a BMP does not.
unsafe fn orientation_of(frame: Com) -> u16 {
    unsafe {
        let get_reader: unsafe extern "system" fn(Com, *mut Com) -> HRESULT = slot(frame, FRAME_GET_METADATA_QUERY_READER);
        let mut reader: Com = null_mut();
        if get_reader(frame, &mut reader) < 0 || reader.is_null() {
            return 1;
        }
        let reader = Obj(reader);
        let by_name: unsafe extern "system" fn(Com, *const u16, *mut PropVariant) -> HRESULT = slot(reader.0, READER_GET_METADATA_BY_NAME);
        // tag 274 in a JPEG's Exif, and in a TIFF's own first directory
        for query in ["/app1/ifd/{ushort=274}", "/ifd/{ushort=274}"] {
            let mut value = PropVariant::default();
            if by_name(reader.0, sys::wide(query).as_ptr(), &mut value) < 0 {
                continue;
            }
            let found = if value.vt == VT_UI2 { (value.value[0] & 0xFFFF) as u16 } else { 0 };
            if value.vt != VT_UI2 {
                clear(&mut value);
            }
            if (1..=8).contains(&found) {
                return found;
            }
        }
        1
    }
}

/// PropVariantClear, for a value that may hold memory of its own.
unsafe fn clear(value: &mut PropVariant) {
    type Clear = unsafe extern "system" fn(*mut PropVariant) -> HRESULT;
    unsafe {
        if let Some(f) = sys::proc_address("ole32.dll", b"PropVariantClear\0") {
            core::mem::transmute::<*const c_void, Clear>(f)(value);
        }
    }
}

/// The frame scaled to `w` x `h` and `part` of it copied out, premultiplied:
/// scaled first and then made 32-bit if `scale_first`, the other way round
/// if not.
unsafe fn pixels(factory: Com, frame: Com, w: u32, h: u32, part: WicRect, scale_first: bool) -> Option<Vec<u32>> {
    unsafe {
        let make = |index: usize| -> Option<Obj> {
            let create: unsafe extern "system" fn(Com, *mut Com) -> HRESULT = slot(factory, index);
            let mut out: Com = null_mut();
            (create(factory, &mut out) >= 0 && !out.is_null()).then(|| Obj(out))
        };
        let scaler = make(FACTORY_CREATE_BITMAP_SCALER)?;
        let converter = make(FACTORY_CREATE_FORMAT_CONVERTER)?;
        type Scale = unsafe extern "system" fn(Com, Com, u32, u32, u32) -> HRESULT;
        type Convert = unsafe extern "system" fn(Com, Com, *const GUID, u32, Com, f64, u32) -> HRESULT;
        let scale: Scale = slot(scaler.0, SCALER_INITIALIZE);
        let convert: Convert = slot(converter.0, CONVERTER_INITIALIZE);
        let last = if scale_first {
            if scale(scaler.0, frame, w, h, WIC_INTERPOLATION_FANT) < 0
                || convert(converter.0, scaler.0, &PIXEL_PBGRA, WIC_DITHER_NONE, null_mut(), 0.0, WIC_PALETTE_CUSTOM) < 0
            {
                return None;
            }
            &converter
        } else {
            if convert(converter.0, frame, &PIXEL_PBGRA, WIC_DITHER_NONE, null_mut(), 0.0, WIC_PALETTE_CUSTOM) < 0
                || scale(scaler.0, converter.0, w, h, WIC_INTERPOLATION_FANT) < 0
            {
                return None;
            }
            &scaler
        };
        let mut px = vec![0u32; (part.w * part.h) as usize];
        let copy: unsafe extern "system" fn(Com, *const WicRect, u32, u32, *mut u8) -> HRESULT = slot(last.0, SOURCE_COPY_PIXELS);
        let stride = part.w as u32 * 4;
        (copy(last.0, &part, stride, stride * part.h as u32, px.as_mut_ptr() as *mut u8) >= 0).then_some(px)
    }
}

/// For the tests, here and in the photo frame's: a 24-bit BMP of `w` x `h`
/// from a colour (0xRRGGBB) for each point, the top row first.
#[cfg(test)]
pub fn bmp(w: usize, h: usize, px: impl Fn(usize, usize) -> u32) -> Vec<u8> {
    let row = (w * 3).div_ceil(4) * 4;
    let size = row * h;
    let mut out = Vec::new();
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(54 + size as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(-(h as i32)).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for y in 0..h {
        for x in 0..w {
            let c = px(x, y);
            out.extend_from_slice(&[c as u8, (c >> 8) as u8, (c >> 16) as u8]);
        }
        out.resize(out.len() + row - w * 3, 0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_scaled_to_cover_the_shape_and_its_middle_cut_out() {
        // a wide photo into the wide frame: as wide, and a little cut off
        // the top and the bottom
        assert_eq!(cover(4000, 3000, 101, 80), (107, 80, [3, 0, 101, 80]));
        // the same photo into the tall frame: its sides go
        assert_eq!(cover(4000, 3000, 80, 101), (135, 101, [27, 0, 80, 101]));
        // into the square: the middle square
        assert_eq!(cover(4000, 3000, 80, 80), (107, 80, [13, 0, 80, 80]));
        // a standing photo into the wide frame: its top and its foot go
        assert_eq!(cover(3000, 4000, 101, 80), (101, 135, [0, 27, 101, 80]));
        // one smaller than the frame is made larger
        assert_eq!(cover(50, 40, 101, 80), (101, 81, [0, 0, 101, 80]));
        // never smaller than the shape, however it rounds
        let (w, h, part) = cover(333, 777, 80, 101);
        assert!(w >= 80 && h >= 101 && part[0] + part[2] <= w && part[1] + part[3] <= h);
    }

    /// A 3 x 2 picture: 1 2 3 over 4 5 6.
    const STORED: [u32; 6] = [1, 2, 3, 4, 5, 6];

    #[test]
    fn a_photo_is_turned_the_way_its_exif_says() {
        assert_eq!(orient(&STORED, 3, 2, 1), [1, 2, 3, 4, 5, 6]);
        assert_eq!(orient(&STORED, 3, 2, 2), [3, 2, 1, 6, 5, 4]);
        assert_eq!(orient(&STORED, 3, 2, 3), [6, 5, 4, 3, 2, 1]);
        assert_eq!(orient(&STORED, 3, 2, 4), [4, 5, 6, 1, 2, 3]);
        // 5 to 8 stand it up: 2 wide and 3 high
        assert_eq!(orient(&STORED, 3, 2, 5), [1, 4, 2, 5, 3, 6]);
        // 6: taken with the camera turned right -- turned a quarter clockwise
        assert_eq!(orient(&STORED, 3, 2, 6), [4, 1, 5, 2, 6, 3]);
        assert_eq!(orient(&STORED, 3, 2, 7), [6, 3, 5, 2, 4, 1]);
        // 8: a quarter anticlockwise
        assert_eq!(orient(&STORED, 3, 2, 8), [3, 6, 2, 5, 1, 4]);
    }

    #[test]
    fn what_is_transparent_is_laid_on_the_background() {
        let bg = 0xFF10_2030;
        assert_eq!(over(0xFF11_2233, bg), 0xFF11_2233);
        assert_eq!(over(0x0000_0000, bg), bg);
        // half white, premultiplied, over the background
        assert_eq!(over(0x8080_8080, bg), 0xFF88_9098);
    }

    #[test]
    fn a_photo_is_read_brought_down_and_cut_to_the_shape() {
        let dir = std::env::temp_dir().join(format!("win98-decode-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 400 x 200: a red left half and a blue right half, with a green
        // band 100 px wide down each edge -- which a square cut leaves out
        let file = dir.join("halves.bmp");
        let colour = |x: usize, _y: usize| match x {
            0..100 | 300.. => 0x00FF00,
            100..200 => 0xFF0000,
            _ => 0x0000FF,
        };
        std::fs::write(&file, bmp(400, 200, colour)).unwrap();
        let px = load(&file, 20, 20, 0).expect("WIC could not be had, or could not read a BMP");
        assert_eq!(px.len(), 400);
        let at = |x: usize, y: usize| px[y * 20 + x];
        // the middle square: red on the left, blue on the right, no green
        assert_eq!((at(2, 10), at(17, 10)), (0xFFFF_0000, 0xFF00_00FF));
        assert!(px.iter().all(|&p| p & 0x00FF00 == 0), "the green edges were not cut off");

        // a file that is no picture, and one that is not there
        let junk = dir.join("junk.jpg");
        std::fs::write(&junk, b"not a jpeg").unwrap();
        assert!(load(&junk, 20, 20, 0).is_none());
        assert!(load(&dir.join("missing.png"), 20, 20, 0).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_what_windows_decodes_itself_is_a_photo() {
        for name in ["a.jpg", "b.JPEG", "c.png", "d.bmp", "e.gif", "f.tif", "g.TIFF", "h.jpe"] {
            assert!(is_photo(Path::new(name)), "{name}");
        }
        for name in ["a.heic", "b.webp", "c.cr2", "d.txt", "noext"] {
            assert!(!is_photo(Path::new(name)), "{name}");
        }
    }
}
