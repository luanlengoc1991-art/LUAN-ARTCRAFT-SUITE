//! The COM side: the class factory Explorer (or its isolated `dllhost.exe`) asks for, the
//! thumbnail provider, the app icon badge and the bitmap handed back.
//!
//! Explorer must never be taken down by a file: every entry point catches panics and turns
//! any failure into an error HRESULT, so Explorer just shows the file's normal icon.

use std::cell::RefCell;
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};

use windows::Win32::Foundation::{CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_POINTER, ERROR_SUCCESS, S_FALSE, S_OK};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDIBSection, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits, HBITMAP, HGDIOBJ, ReleaseDC,
};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl, IStream, STREAM_SEEK_END, STREAM_SEEK_SET};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::UI::Shell::PropertiesSystem::{IInitializeWithStream, IInitializeWithStream_Impl};
use windows::Win32::UI::Shell::{IThumbnailProvider, IThumbnailProvider_Impl, WTS_ALPHATYPE, WTSAT_ARGB};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO, PrivateExtractIconsW};
use windows_core::{BOOL, GUID, HRESULT, IUnknown, Interface, PCWSTR, Ref, implement};

use crate::formats::{App, Source};
use crate::image::Rgba;

/// The provider's class id (`scripts/install-thumbs.ps1` registers it).
pub const CLSID: GUID = GUID::from_u128(0xDBC0383E_886F_4B5F_B407_AFEA2F1DF2E0);

/// Where `install-thumbs.ps1` records apps installed outside `%ProgramFiles%` (value `<id>`
/// = the exe's path).
const APPS_KEY: &str = "Software\\ArtCraft\\Thumbnails";

/// Live objects: the DLL may be unloaded when none is left.
static OBJECTS: AtomicUsize = AtomicUsize::new(0);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

#[implement(IThumbnailProvider, IInitializeWithStream)]
struct Provider {
    stream: RefCell<Option<IStream>>,
}

impl Provider {
    fn new() -> Self {
        OBJECTS.fetch_add(1, Ordering::SeqCst);
        Provider { stream: RefCell::new(None) }
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        OBJECTS.fetch_sub(1, Ordering::SeqCst);
    }
}

impl IInitializeWithStream_Impl for Provider_Impl {
    fn Initialize(&self, pstream: Ref<IStream>, _grfmode: u32) -> windows_core::Result<()> {
        let stream = pstream.ok()?.clone();
        *self.stream.try_borrow_mut().map_err(|_| windows_core::Error::from(E_FAIL))? = Some(stream);
        Ok(())
    }
}

impl IThumbnailProvider_Impl for Provider_Impl {
    fn GetThumbnail(&self, cx: u32, phbmp: *mut HBITMAP, pdwalpha: *mut WTS_ALPHATYPE) -> windows_core::Result<()> {
        if phbmp.is_null() || pdwalpha.is_null() {
            return Err(E_POINTER.into());
        }
        let stream = self.stream.try_borrow().ok().and_then(|s| s.clone()).ok_or(windows_core::Error::from(E_FAIL))?;
        let size = cx.clamp(16, 2048);
        let img = catch_unwind(AssertUnwindSafe(|| crate::thumbnail(&mut StreamSource(&stream), size, app_icon)))
            .ok()
            .flatten()
            .ok_or(windows_core::Error::from(E_FAIL))?;
        let bmp = to_hbitmap(&img).ok_or(windows_core::Error::from(E_FAIL))?;
        // SAFETY: both pointers were checked non-null; Explorer owns the bitmap from here.
        unsafe {
            *phbmp = bmp;
            *pdwalpha = WTSAT_ARGB;
        }
        Ok(())
    }
}

/// The file Explorer passed, read through its stream.
struct StreamSource<'a>(&'a IStream);

impl Source for StreamSource<'_> {
    fn size(&mut self) -> Option<u64> {
        let mut end = 0u64;
        // SAFETY: a valid stream and an out pointer to a local.
        unsafe { self.0.Seek(0, STREAM_SEEK_END, Some(&mut end)) }.ok()?;
        Some(end)
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Option<()> {
        // SAFETY: as above.
        unsafe { self.0.Seek(i64::try_from(offset).ok()?, STREAM_SEEK_SET, None) }.ok()?;
        let mut done = 0usize;
        while done < buf.len() {
            let rest = buf.get_mut(done..)?;
            let want = u32::try_from(rest.len()).unwrap_or(1 << 30).min(1 << 30);
            let mut n = 0u32;
            // SAFETY: `rest` has at least `want` writable bytes.
            let hr = unsafe { self.0.Read(rest.as_mut_ptr().cast(), want, Some(&mut n)) };
            if hr.is_err() || n == 0 {
                return None;
            }
            done += n as usize;
        }
        Some(())
    }
}

/// The installed app's exe: the path `install-thumbs.ps1` recorded, else the default install
/// folders.
fn app_exe(app: App) -> Option<String> {
    let mut buf = vec![0u16; 1024];
    let mut bytes = (buf.len() * 2) as u32;
    let (key, value) = (wide(APPS_KEY), wide(app.id()));
    // SAFETY: NUL-terminated names, a buffer of `bytes` bytes.
    let r = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR(value.as_ptr()), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut bytes))
    };
    let mut candidates = Vec::new();
    if r == ERROR_SUCCESS {
        let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
        candidates.push(String::from_utf16_lossy(buf.get(..n)?));
    }
    let exe = format!("{}\\{}.exe", app.name(), app.id());
    for var in ["ProgramFiles", "ProgramW6432"] {
        if let Some(d) = std::env::var_os(var) {
            candidates.push(format!("{}\\{exe}", d.to_string_lossy()));
        }
    }
    if let Some(d) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(format!("{}\\Programs\\{exe}", d.to_string_lossy()));
    }
    candidates.into_iter().find(|p| !p.is_empty() && std::path::Path::new(p).is_file())
}

/// The app's icon at `side`×`side`, from its exe.
fn app_icon(app: App, side: u32) -> Option<Rgba> {
    let exe = app_exe(app)?;
    let mut name = [0u16; 260];
    let w: Vec<u16> = exe.encode_utf16().collect();
    name.get_mut(..w.len())?.copy_from_slice(&w);
    let mut icons = [HICON::default()];
    let s = side as i32;
    // SAFETY: a NUL-terminated path and room for one icon.
    let n = unsafe { PrivateExtractIconsW(&name, 0, s, s, Some(&mut icons), None, 0) };
    if n == 0 || n == u32::MAX || icons[0].is_invalid() {
        return None;
    }
    let img = icon_pixels(icons[0], side);
    // SAFETY: we own the icon.
    let _ = unsafe { DestroyIcon(icons[0]) }; // Nothing to do if it fails.
    img
}

/// 32-bit top-down DIB header for `w`×`h`.
fn header(w: u32, h: u32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// An icon's pixels as straight RGBA.
fn icon_pixels(icon: HICON, side: u32) -> Option<Rgba> {
    let mut info = ICONINFO::default();
    // SAFETY: a valid icon; the bitmaps it returns are ours to delete.
    unsafe { GetIconInfo(icon, &mut info) }.ok()?;
    let n = (side * side) as usize;
    let mut bgra = vec![0u8; n * 4];
    let mut mask = vec![0u8; n * 4];
    // SAFETY: the screen DC, released below; buffers sized for a 32-bit side×side DIB.
    unsafe {
        let dc = GetDC(None);
        let mut bi = header(side, side);
        let c = !info.hbmColor.is_invalid() && GetDIBits(dc, info.hbmColor, 0, side, Some(bgra.as_mut_ptr().cast()), &mut bi, DIB_RGB_COLORS) == side as i32;
        if !info.hbmMask.is_invalid() {
            // A mask that can't be read stays zero: every pixel opaque.
            let mut bi = header(side, side);
            GetDIBits(dc, info.hbmMask, 0, side, Some(mask.as_mut_ptr().cast()), &mut bi, DIB_RGB_COLORS);
        }
        ReleaseDC(None, dc);
        let _ = DeleteObject(HGDIOBJ(info.hbmColor.0)); // Freeing; nothing to do on failure.
        let _ = DeleteObject(HGDIOBJ(info.hbmMask.0));
        c.then_some(())
    }?;
    let has_alpha = bgra.chunks_exact(4).any(|p| p[3] != 0);
    let pixels = bgra
        .chunks_exact(4)
        .zip(mask.chunks_exact(4))
        .flat_map(|(p, m)| {
            // Icons without alpha: the AND mask says which pixels are transparent.
            let a = if has_alpha { p[3] } else if mask_ok(m) { 0 } else { 255 };
            [p[2], p[1], p[0], a]
        })
        .collect();
    Some(Rgba { width: side, height: side, pixels })
}

fn mask_ok(m: &[u8]) -> bool {
    m.first().is_some_and(|&b| b != 0)
}

/// A 32-bit ARGB DIB section of `img` (straight alpha, as `WTSAT_ARGB` expects).
fn to_hbitmap(img: &Rgba) -> Option<HBITMAP> {
    let bi = header(img.width, img.height);
    let mut bits: *mut c_void = std::ptr::null_mut();
    // SAFETY: a valid header; `bits` receives the section's memory.
    let bmp = unsafe { CreateDIBSection(None, &bi, DIB_RGB_COLORS, &mut bits, None, 0) }.ok()?;
    if bits.is_null() {
        // SAFETY: our bitmap.
        let _ = unsafe { DeleteObject(HGDIOBJ(bmp.0)) };
        return None;
    }
    // SAFETY: the section holds width×height 32-bit pixels.
    let out = unsafe { std::slice::from_raw_parts_mut(bits.cast::<u8>(), img.pixels.len()) };
    for (d, s) in out.chunks_exact_mut(4).zip(img.pixels.chunks_exact(4)) {
        d.copy_from_slice(&[s[2], s[1], s[0], s[3]]);
    }
    Some(bmp)
}

#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(&self, punkouter: Ref<IUnknown>, riid: *const GUID, ppvobject: *mut *mut c_void) -> windows_core::Result<()> {
        if ppvobject.is_null() || riid.is_null() {
            return Err(E_POINTER.into());
        }
        // SAFETY: checked non-null.
        unsafe { *ppvobject = std::ptr::null_mut() };
        if !punkouter.is_null() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let unknown: IUnknown = Provider::new().into();
        // SAFETY: valid riid and out pointer.
        unsafe { unknown.query(riid, ppvobject) }.ok()
    }

    fn LockServer(&self, flock: BOOL) -> windows_core::Result<()> {
        if flock.as_bool() {
            OBJECTS.fetch_add(1, Ordering::SeqCst);
        } else {
            OBJECTS.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(())
    }
}

/// COM entry point: the class factory for [`CLSID`].
///
/// # Safety
/// Called by COM with valid pointers.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(rclsid: *const GUID, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    catch_unwind(|| {
        if rclsid.is_null() || riid.is_null() || ppv.is_null() {
            return E_POINTER;
        }
        // SAFETY: checked non-null.
        unsafe {
            *ppv = std::ptr::null_mut();
            if *rclsid != CLSID {
                return CLASS_E_CLASSNOTAVAILABLE;
            }
            let factory: IClassFactory = Factory.into();
            factory.query(riid, ppv)
        }
    })
    .unwrap_or(E_FAIL)
}

/// COM entry point: may the DLL be unloaded?
#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if OBJECTS.load(Ordering::SeqCst) == 0 { S_OK } else { S_FALSE }
}
