use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
fn main() {
    let path: Vec<u16> = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe\0".encode_utf16().collect();
    unsafe {
        let mut sfi = SHFILEINFOW::default();
        let r = SHGetFileInfoW(PCWSTR(path.as_ptr()), windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0), Some(&mut sfi), std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_ICON | SHGFI_SMALLICON);
        println!("shgfi r={} hicon={:?}", r, sfi.hIcon);
        let mut ii = ICONINFO::default();
        let ok = GetIconInfo(sfi.hIcon, &mut ii);
        println!("geticoninfo {:?} color={:?} mask={:?}", ok, ii.hbmColor, ii.hbmMask);
        let mut bm = BITMAP::default();
        let n = GetObjectW(ii.hbmColor.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _));
        println!("getobject {} {}x{} bpp={}", n, bm.bmWidth, bm.bmHeight, bm.bmBitsPixel);
        println!("b64 len {:?}", pulse_lib::actions::icon_png_base64("C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe").map(|s| s.len()));
    }
}
