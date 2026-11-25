use windows::core::PCWSTR;
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::UI::Shell::{
    ExtractIconExW, Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_WARNING,
    NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS,
};
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow, GetSystemMetricsForDpi};

use crate::native_interop::WM_APP_TRAY;
pub use crate::ring_icon::RingValues;
use crate::ring_icon;

const APP_TRAY_ICON_ID: u32 = 1;
const LEGACY_CODEX_TRAY_ICON_ID: u32 = 2;
const LEGACY_ANTIGRAVITY_TRAY_ICON_ID: u32 = 3;

/// Menu item ID for toggling widget visibility (used by window.rs context menu).
pub const IDM_TOGGLE_WIDGET: u16 = 70;

/// Actions the tray message handler can request from the main window.
pub enum TrayAction {
    None,
    ToggleWidget,
    ShowContextMenu,
}

/// Identifies the provider that originated a notification. All notifications
/// now use the one application tray icon rather than provider-specific icons.
#[derive(Clone, Copy)]
pub enum TrayIconKind {
    Claude,
    Codex,
    Antigravity,
}

pub struct TrayIconData {
    pub tooltip: String,
    /// Ring gauge values; `None` falls back to the embedded app icon.
    pub ring: Option<RingValues>,
}

/// Load the same embedded icon used by the executable and desktop shortcut.
pub fn create_icon() -> HICON {
    load_embedded_app_icon()
}

fn tray_icon_size(hwnd: HWND) -> i32 {
    unsafe {
        let dpi = GetDpiForWindow(hwnd);
        let dpi = if dpi == 0 { GetDpiForSystem() } else { dpi };
        let size = GetSystemMetricsForDpi(SM_CXSMICON, dpi);
        if size > 0 {
            size
        } else {
            16
        }
    }
}

/// Render the split ring gauge (top: 5-hour, bottom: weekly) as an HICON.
pub fn create_ring_icon(hwnd: HWND, values: RingValues) -> HICON {
    let size = tray_icon_size(hwnd).clamp(12, 64);
    let pixels = ring_icon::render_bgra(size as u32, values, crate::theme::is_dark_mode());
    unsafe {
        let header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let Ok(color) = CreateDIBSection(None, &header, DIB_RGB_COLORS, &mut bits, None, 0) else {
            return load_embedded_app_icon();
        };
        if bits.is_null() {
            let _ = DeleteObject(color);
            return load_embedded_app_icon();
        }
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, pixels.len());
        let mask_bytes = vec![0u8; (((size + 15) / 16) * 2 * size) as usize];
        let mask = CreateBitmap(size, size, 1, 1, Some(mask_bytes.as_ptr() as *const _));
        let info = ICONINFO {
            fIcon: TRUE,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&info).unwrap_or_default();
        let _ = DeleteObject(color);
        let _ = DeleteObject(mask);
        if icon.is_invalid() {
            load_embedded_app_icon()
        } else {
            icon
        }
    }
}

fn icon_for(hwnd: HWND, ring: Option<RingValues>) -> HICON {
    match ring {
        Some(values) => create_ring_icon(hwnd, values),
        None => create_icon(),
    }
}

fn load_embedded_app_icon() -> HICON {
    unsafe {
        let mut exe_buf = [0u16; 260];
        let len = GetModuleFileNameW(None, &mut exe_buf) as usize;
        if len == 0 {
            return HICON::default();
        }

        let mut small_icon = HICON::default();
        let mut large_icon = HICON::default();
        let extracted = ExtractIconExW(
            PCWSTR::from_raw(exe_buf.as_ptr()),
            0,
            Some(&mut large_icon),
            Some(&mut small_icon),
            1,
        );

        if extracted == 0 {
            return HICON::default();
        }

        if !small_icon.is_invalid() {
            if !large_icon.is_invalid() {
                let _ = DestroyIcon(large_icon);
            }
            small_icon
        } else {
            large_icon
        }
    }
}

/// Show a Windows balloon notification from the single application tray icon.
pub fn notify_balloon(hwnd: HWND, _kind: TrayIconKind, title: &str, message: &str) {
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = APP_TRAY_ICON_ID;
        nid.uFlags = NIF_INFO;
        nid.dwInfoFlags = NIIF_WARNING;
        copy_wide(title, &mut nid.szInfoTitle);
        copy_wide(message, &mut nid.szInfo);
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

fn copy_wide<const N: usize>(s: &str, buf: &mut [u16; N]) {
    let wide: Vec<u16> = s.encode_utf16().collect();
    let mut len = wide.len().min(N - 1);
    if len > 0 && (0xD800..=0xDBFF).contains(&wide[len - 1]) {
        len -= 1;
    }
    buf[..len].copy_from_slice(&wide[..len]);
    buf[len] = 0;
}

fn add(hwnd: HWND, tooltip: &str, ring: Option<RingValues>) {
    let hicon = icon_for(hwnd, ring);
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = APP_TRAY_ICON_ID;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_APP_TRAY;
        nid.hIcon = hicon;
        copy_wide(tooltip, &mut nid.szTip);
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        if !hicon.is_invalid() {
            let _ = DestroyIcon(hicon);
        }
    }
}

fn update(hwnd: HWND, tooltip: &str, ring: Option<RingValues>) {
    let hicon = icon_for(hwnd, ring);
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = APP_TRAY_ICON_ID;
        nid.uFlags = NIF_ICON | NIF_TIP;
        nid.hIcon = hicon;
        copy_wide(tooltip, &mut nid.szTip);
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        if !hicon.is_invalid() {
            let _ = DestroyIcon(hicon);
        }
    }
}

fn remove_id(hwnd: HWND, id: u32) {
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = id;
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

pub fn sync(hwnd: HWND, icon: Option<&TrayIconData>) {
    // Remove provider-specific icons left by versions before v1.9.
    remove_id(hwnd, LEGACY_CODEX_TRAY_ICON_ID);
    remove_id(hwnd, LEGACY_ANTIGRAVITY_TRAY_ICON_ID);

    if let Some(icon) = icon {
        add(hwnd, &icon.tooltip, icon.ring);
        update(hwnd, &icon.tooltip, icon.ring);
    } else {
        remove_id(hwnd, APP_TRAY_ICON_ID);
    }
}

pub fn remove_all(hwnd: HWND) {
    remove_id(hwnd, APP_TRAY_ICON_ID);
    remove_id(hwnd, LEGACY_CODEX_TRAY_ICON_ID);
    remove_id(hwnd, LEGACY_ANTIGRAVITY_TRAY_ICON_ID);
}

/// Interpret a tray callback message and return the action to take.
pub fn handle_message(lparam: LPARAM) -> TrayAction {
    let mouse_msg = lparam.0 as u32;
    match mouse_msg {
        WM_LBUTTONUP => TrayAction::ToggleWidget,
        WM_RBUTTONUP => TrayAction::ShowContextMenu,
        _ => TrayAction::None,
    }
}
