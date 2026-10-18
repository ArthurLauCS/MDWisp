//! Frameless top-level windows: the app draws its own chrome but keeps the
//! system behaviours — drag, resize borders, Aero Snap, the Windows 11 snap
//! layout flyout on the maximise button, DWM shadow and rounded corners.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{MARGINS, WM_MOUSELEAVE};
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::ui::{Cursor, Hit};
use crate::window::Window;

pub const CLASS: PCWSTR = w!("MDWispWindow");
/// Background work finished; lparam is a `Box<Box<dyn FnOnce(&mut Window)>>`.
pub const WM_APP_RUN: u32 = WM_APP + 2;

pub fn register(bg: u32) {
    unsafe {
        let inst = GetModuleHandleW(None).unwrap();
        RegisterClassExW(&WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: Some(proc),
            hInstance: inst.into(),
            lpszClassName: CLASS,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap(),
            hIcon: LoadIconW(Some(inst.into()), PCWSTR(1 as _)).unwrap_or_default(),
            // Painted by the system before the first frame exists, so the
            // window never flashes white while the device warms up.
            hbrBackground: CreateSolidBrush(COLORREF(bg)),
            ..Default::default()
        });
    }
}

pub fn create(w: Box<Window>, x: i32, y: i32, cw: i32, ch: i32, maximized: bool) -> HWND {
    unsafe {
        let inst = GetModuleHandleW(None).unwrap();
        let ptr = Box::into_raw(w);
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP,
            CLASS,
            w!("MDWisp"),
            WS_OVERLAPPEDWINDOW,
            x,
            y,
            cw,
            ch,
            None,
            None,
            Some(inst.into()),
            Some(ptr as _),
        )
        .unwrap();
        let dark = BOOL(1);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const _ as _, 4);
        let round = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &round as *const _ as _, 4);
        // A one-pixel sliver of DWM frame keeps the system shadow.
        let _ = DwmExtendFrameIntoClientArea(hwnd, &MARGINS { cxLeftWidth: 0, cxRightWidth: 0, cyTopHeight: 1, cyBottomHeight: 0 });
        SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER).unwrap();
        if let Some(w) = window(hwnd) {
            w.created();
        }
        let _ = ShowWindow(hwnd, if maximized { SW_SHOWMAXIMIZED } else { SW_SHOW });
        hwnd
    }
}

pub fn window(hwnd: HWND) -> Option<&'static mut Window> {
    unsafe {
        let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Window;
        p.as_mut()
    }
}

pub fn is_maximized(hwnd: HWND) -> bool {
    unsafe { IsZoomed(hwnd).as_bool() }
}

pub fn dpi_of(hwnd: HWND) -> f32 {
    unsafe { GetDpiForWindow(hwnd) as f32 }
}

fn lo(l: LPARAM) -> i32 {
    (l.0 & 0xffff) as i16 as i32
}

fn hi(l: LPARAM) -> i32 {
    ((l.0 >> 16) & 0xffff) as i16 as i32
}

pub fn mods() -> crate::keys::Mods {
    unsafe {
        let down = |k: VIRTUAL_KEY| GetKeyState(k.0 as i32) < 0;
        crate::keys::Mods { ctrl: down(VK_CONTROL), shift: down(VK_SHIFT), alt: down(VK_MENU) }
    }
}

/// Run `f` on the UI thread. Takes the raw handle so worker threads can carry it.
pub fn post(hwnd: isize, f: impl FnOnce(&mut Window) + Send + 'static) {
    let hwnd = HWND(hwnd as _);
    let b: Box<Box<dyn FnOnce(&mut Window) + Send>> = Box::new(Box::new(f));
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_APP_RUN, WPARAM(0), LPARAM(Box::into_raw(b) as isize));
    }
}

pub fn request_frame(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

pub fn set_cursor(c: Cursor) {
    let id = match c {
        Cursor::Arrow => IDC_ARROW,
        Cursor::Hand => IDC_HAND,
        Cursor::IBeam => IDC_IBEAM,
        Cursor::ResizeH => IDC_SIZEWE,
        Cursor::ResizeV => IDC_SIZENS,
    };
    unsafe {
        SetCursor(LoadCursorW(None, id).ok());
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_NCCREATE {
            let cs = &*(lp.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
            (*(cs.lpCreateParams as *mut Window)).hwnd = hwnd;
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        let Some(w) = window(hwnd) else { return DefWindowProcW(hwnd, msg, wp, lp) };
        match msg {
            WM_NCCALCSIZE if wp.0 != 0 => {
                // The client area is the whole window. Maximised windows hang
                // their frame off-screen, so pull the client back inside.
                if is_maximized(hwnd) {
                    let p = &mut *(lp.0 as *mut NCCALCSIZE_PARAMS);
                    let dpi = GetDpiForWindow(hwnd);
                    let f = GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
                    p.rgrc[0].left += f;
                    p.rgrc[0].right -= f;
                    p.rgrc[0].top += f;
                    p.rgrc[0].bottom -= f;
                }
                LRESULT(0)
            }
            WM_NCHITTEST => {
                let mut pt = POINT { x: lo(lp), y: hi(lp) };
                let _ = ScreenToClient(hwnd, &mut pt);
                let s = dpi_of(hwnd) / 96.0;
                let (x, y) = (pt.x as f32 / s, pt.y as f32 / s);
                if !is_maximized(hwnd) {
                    let b = 6.0;
                    let (left, right, top, bottom) = (x < b, x >= w.width - b, y < b, y >= w.height - b);
                    let ht = match (left, right, top, bottom) {
                        (true, _, true, _) => HTTOPLEFT,
                        (_, true, true, _) => HTTOPRIGHT,
                        (true, _, _, true) => HTBOTTOMLEFT,
                        (_, true, _, true) => HTBOTTOMRIGHT,
                        (true, ..) => HTLEFT,
                        (_, true, ..) => HTRIGHT,
                        (_, _, true, _) => HTTOP,
                        (.., true) => HTBOTTOM,
                        _ => HTNOWHERE,
                    };
                    if ht != HTNOWHERE {
                        return LRESULT(ht as isize);
                    }
                }
                LRESULT(match w.ui.hit_at(x, y) {
                    Some(Hit::Caption) => HTCAPTION,
                    // The snap layout flyout only appears for HTMAXBUTTON.
                    Some(Hit::Maximize) => HTMAXBUTTON,
                    _ => HTCLIENT,
                } as isize)
            }
            WM_NCMOUSEMOVE | WM_NCMOUSEHOVER => {
                let over_max = wp.0 == HTMAXBUTTON as usize;
                let mut pt = POINT { x: lo(lp), y: hi(lp) };
                let _ = ScreenToClient(hwnd, &mut pt);
                let s = dpi_of(hwnd) / 96.0;
                w.pointer_move(pt.x as f32 / s, pt.y as f32 / s, over_max);
                if over_max {
                    let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE | TME_NONCLIENT, hwndTrack: hwnd, dwHoverTime: 0 };
                    let _ = TrackMouseEvent(&mut tme);
                    return LRESULT(0);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_NCMOUSELEAVE => {
                w.pointer_leave();
                LRESULT(0)
            }
            WM_NCLBUTTONDOWN if wp.0 == HTMAXBUTTON as usize => {
                w.ui.press = Some(Hit::Maximize);
                request_frame(hwnd);
                LRESULT(0)
            }
            WM_NCLBUTTONUP if wp.0 == HTMAXBUTTON as usize => {
                if w.ui.press.take() == Some(Hit::Maximize) {
                    w.toggle_maximize();
                }
                LRESULT(0)
            }
            WM_NCLBUTTONDBLCLK if wp.0 == HTMAXBUTTON as usize => LRESULT(0),
            WM_MOUSEMOVE => {
                let s = dpi_of(hwnd) / 96.0;
                if !w.ui.inside {
                    let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    let _ = TrackMouseEvent(&mut tme);
                }
                w.pointer_move(lo(lp) as f32 / s, hi(lp) as f32 / s, false);
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                w.pointer_leave();
                LRESULT(0)
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
                let s = dpi_of(hwnd) / 96.0;
                SetCapture(hwnd);
                let button = match msg {
                    WM_RBUTTONDOWN => 1,
                    WM_MBUTTONDOWN => 2,
                    _ => 0,
                };
                w.pointer_down(lo(lp) as f32 / s, hi(lp) as f32 / s, button, msg == WM_LBUTTONDBLCLK);
                LRESULT(0)
            }
            WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP => {
                let s = dpi_of(hwnd) / 96.0;
                let _ = ReleaseCapture();
                let button = match msg {
                    WM_RBUTTONUP => 1,
                    WM_MBUTTONUP => 2,
                    _ => 0,
                };
                w.pointer_up(lo(lp) as f32 / s, hi(lp) as f32 / s, button);
                LRESULT(0)
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                let delta = ((wp.0 >> 16) & 0xffff) as i16 as f32 / 120.0;
                w.wheel(if msg == WM_MOUSEWHEEL { delta } else { 0.0 }, if msg == WM_MOUSEHWHEEL { delta } else { 0.0 });
                LRESULT(0)
            }
            WM_SETCURSOR if (lp.0 & 0xffff) as u32 == HTCLIENT => {
                set_cursor(w.cursor());
                LRESULT(1)
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                if w.key_down(VIRTUAL_KEY(wp.0 as u16), mods()) {
                    LRESULT(0)
                } else {
                    DefWindowProcW(hwnd, msg, wp, lp)
                }
            }
            WM_KEYUP | WM_SYSKEYUP => DefWindowProcW(hwnd, msg, wp, lp),
            WM_CHAR => {
                w.char_input(wp.0 as u16);
                LRESULT(0)
            }
            // Alt alone would open the (absent) system menu and swallow the next key.
            WM_SYSCHAR => LRESULT(0),
            WM_IME_STARTCOMPOSITION | WM_IME_COMPOSITION | WM_IME_ENDCOMPOSITION => {
                if w.ime(msg, lp) { LRESULT(0) } else { DefWindowProcW(hwnd, msg, wp, lp) }
            }
            WM_SETFOCUS => {
                w.focus_changed(true);
                LRESULT(0)
            }
            WM_KILLFOCUS => {
                w.focus_changed(false);
                LRESULT(0)
            }
            WM_SIZE => {
                let s = dpi_of(hwnd) / 96.0;
                w.resized(lo(lp) as u32, hi(lp) as u32, s);
                LRESULT(0)
            }
            WM_DPICHANGED => {
                let r = &*(lp.0 as *const RECT);
                SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE).unwrap();
                LRESULT(0)
            }
            WM_GETMINMAXINFO => {
                let mm = &mut *(lp.0 as *mut MINMAXINFO);
                let s = dpi_of(hwnd) / 96.0;
                mm.ptMinTrackSize = POINT { x: (720.0 * s) as i32, y: (520.0 * s) as i32 };
                LRESULT(0)
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut ps);
                let _ = EndPaint(hwnd, &ps);
                w.paint();
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_TIMER => {
                w.timer(wp.0);
                LRESULT(0)
            }
            WM_APP_RUN => {
                let f = Box::from_raw(lp.0 as *mut Box<dyn FnOnce(&mut Window) + Send>);
                f(w);
                LRESULT(0)
            }
            crate::editor::images::WM_APP_IMAGE => {
                w.images_ready();
                LRESULT(0)
            }
            crate::watch::WM_APP_DISK => {
                w.disk_changed();
                LRESULT(0)
            }
            WM_DROPFILES => {
                w.drop_files(windows::Win32::UI::Shell::HDROP(wp.0 as _));
                LRESULT(0)
            }
            WM_CLOSE => {
                if w.request_close() {
                    let _ = DestroyWindow(hwnd);
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(w as *mut Window));
                crate::app().window_closed(hwnd);
                LRESULT(0)
            }
            WM_COPYDATA => {
                crate::app().forwarded(&*(lp.0 as *const windows::Win32::System::DataExchange::COPYDATASTRUCT));
                LRESULT(1)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
