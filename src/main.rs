#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cache;
mod config;
mod language;
mod translator;

#[cfg(not(target_os = "windows"))]
compile_error!("fy 仅支持 Windows");

use std::{
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
    sync::{Mutex, OnceLock},
    thread,
};

use anyhow::{Context, Result, bail};
use config::{Config, Paths, WindowPosition};
use windows_sys::Win32::{
    Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Dwm::{
        DWM_WINDOW_CORNER_PREFERENCE, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
    },
    Graphics::Gdi::{
        BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, DC_BRUSH, DC_PEN,
        DEFAULT_CHARSET, DEFAULT_GUI_FONT, DEFAULT_PITCH, DT_LEFT, DT_SINGLELINE, DT_VCENTER,
        DeleteObject, DrawTextW, EndPaint, FW_NORMAL, FillRect, GetDC, GetMonitorInfoW,
        GetStockObject, GetTextMetricsW, HFONT, InvalidateRect, LineTo, MONITOR_DEFAULTTONEAREST,
        MONITORINFO, MonitorFromPoint, MoveToEx, NULL_PEN, OUT_DEFAULT_PRECIS, PAINTSTRUCT,
        ReleaseDC, RoundRect, SelectObject, SetBkColor, SetBkMode, SetDCBrushColor, SetDCPenColor,
        SetTextColor, TEXTMETRICW, TRANSPARENT,
    },
    System::{
        LibraryLoader::GetModuleHandleW,
        Registry::{
            HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
            RegCreateKeyExW, RegDeleteValueW, RegSetValueExW,
        },
    },
    UI::{
        Controls::{
            EM_GETFIRSTVISIBLELINE, EM_GETLINECOUNT, EM_LINESCROLL, EM_REPLACESEL, EM_SETSEL,
            SetWindowTheme, WM_MOUSELEAVE,
        },
        HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
        Input::KeyboardAndMouse::{
            GetKeyState, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
            ReleaseCapture, SetCapture, SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
            UnregisterHotKey, VK_CONTROL, VK_ESCAPE, VK_RETURN,
        },
        Shell::{
            DefSubclassProc, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
            SetWindowSubclass, Shell_NotifyIconW, ShellExecuteW,
        },
        WindowsAndMessaging::{
            AppendMenuW, CS_DROPSHADOW, CW_USEDEFAULT, CreateIconFromResourceEx, CreatePopupMenu,
            CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu, DispatchMessageW,
            ES_AUTOVSCROLL, ES_MULTILINE, ES_READONLY, ES_WANTRETURN, GetClientRect, GetCursorPos,
            GetDlgItem, GetMessageW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, HICON,
            HMENU, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTLEFT, HTRIGHT, HTTOP,
            HTTOPLEFT, HTTOPRIGHT, HWND_NOTOPMOST, HWND_TOPMOST, IDC_ARROW, IsWindowVisible,
            KillTimer, LR_DEFAULTCOLOR, LoadCursorW, MB_ICONERROR, MB_OK, MF_CHECKED, MF_POPUP,
            MF_SEPARATOR, MF_STRING, MINMAXINFO, MSG, MessageBoxW, PostMessageW, PostQuitMessage,
            RegisterClassW, SW_HIDE, SW_SHOW, SW_SHOWNORMAL, SWP_NOMOVE, SWP_NOSIZE,
            SetForegroundWindow, SetTimer, SetWindowPos, SetWindowTextW, ShowWindow,
            TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage,
            WA_INACTIVE, WM_ACTIVATE, WM_APP, WM_CLOSE, WM_COMMAND, WM_CTLCOLOREDIT,
            WM_CTLCOLORSTATIC, WM_DESTROY, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_HOTKEY, WM_KEYDOWN,
            WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
            WM_NCHITTEST, WM_NCLBUTTONDOWN, WM_PAINT, WM_RBUTTONUP, WM_SETFONT, WM_SIZE, WM_TIMER,
            WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
            WS_TABSTOP, WS_VISIBLE,
        },
    },
};

const HOTKEY_ID: i32 = 1;
const TRAY_ID: u32 = 1;
const WM_TRAY: u32 = WM_APP + 1;
const WM_CAPTURED: u32 = WM_APP + 2;
const WM_TRANSLATION_DELTA: u32 = WM_APP + 3;
const WM_TRANSLATED: u32 = WM_APP + 4;
const WM_MANUAL_TRANSLATE: u32 = WM_APP + 5;
const SPINNER_TIMER_ID: usize = 1;
const MK_LBUTTON_STATE: usize = 0x0001;
const MENU_SHOW: usize = 1001;
const MENU_RELOAD: usize = 1002;
const MENU_FOLDER: usize = 1003;
const MENU_EXIT: usize = 1004;
const MENU_PROVIDER_BASE: usize = 2000;
const ICON_BYTES: &[u8] = include_bytes!("../assets/fy.ico");
const INPUT_ID: i32 = 1;
const OUTPUT_ID: i32 = 2;
const HEADER_HEIGHT: i32 = 44;
const MIN_WINDOW_WIDTH: i32 = 360;
const MIN_WINDOW_HEIGHT: i32 = 300;
const COLOR_WINDOW: COLORREF = rgb(32, 32, 32);
const COLOR_CARD: COLORREF = rgb(42, 42, 42);
const COLOR_BUTTON: COLORREF = rgb(52, 52, 52);
const COLOR_ACCENT: COLORREF = rgb(59, 130, 246);
const COLOR_TEXT: COLORREF = rgb(242, 242, 242);
const COLOR_MUTED: COLORREF = rgb(170, 170, 170);

const fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    red as u32 | ((green as u32) << 8) | ((blue as u32) << 16)
}

static APP: OnceLock<Mutex<App>> = OnceLock::new();

struct App {
    hwnd: HWND,
    input: HWND,
    output: HWND,
    config: Config,
    paths: Paths,
    request_id: u64,
    translating: bool,
    streamed_text: String,
    pinned: bool,
    split_ratio: f32,
    dragging_splitter: bool,
    scrollbar_hover: HWND,
    scrollbar_drag: HWND,
    spinner_frame: u8,
    tray: NOTIFYICONDATAW,
    icon: HICON,
    font: HFONT,
}

// HWND 仅由创建它的 UI 线程使用；Mutex 只为 Win32 回调提供内部可变性。
unsafe impl Send for App {}

fn main() {
    if let Err(error) = run() {
        show_error(null_mut(), &format!("fy 启动失败\n\n{error:#}"));
    }
}

fn run() -> Result<()> {
    let (config, paths) = Config::load_or_create()?;
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let icon = unsafe { load_embedded_icon()? };
    let (hwnd, input, output, font) = match unsafe { create_window(&config, icon) } {
        Ok(window) => window,
        Err(error) => {
            unsafe { DestroyIcon(icon) };
            return Err(error);
        }
    };
    let tray = match unsafe { add_tray_icon(hwnd, icon) } {
        Ok(tray) => tray,
        Err(error) => {
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
                DeleteObject(font);
                DestroyIcon(icon);
            }
            return Err(error);
        }
    };
    let pinned = config.app.always_on_top;
    let split_ratio = config.app.input_ratio;
    APP.set(Mutex::new(App {
        hwnd,
        input,
        output,
        config,
        paths,
        request_id: 0,
        translating: false,
        streamed_text: String::new(),
        pinned,
        split_ratio,
        dragging_splitter: false,
        scrollbar_hover: null_mut(),
        scrollbar_drag: null_mut(),
        spinner_frame: 0,
        tray,
        icon,
        font,
    }))
    .map_err(|_| anyhow::anyhow!("应用状态重复初始化"))?;

    // CreateWindowExW 可能在 EDIT 子控件创建前发送首次 WM_SIZE，主动完成初始布局。
    resize_controls(hwnd);

    {
        let mut app = APP.get().unwrap().lock().unwrap();
        register_hotkey(&mut app)?;
        apply_autostart(&app.config)?;
    }

    unsafe {
        let mut message: MSG = zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

unsafe fn load_embedded_icon() -> Result<HICON> {
    let resource = embedded_icon_resource(32)?;
    let resource_size = u32::try_from(resource.len()).context("内嵌图标资源过大")?;
    let icon = unsafe {
        CreateIconFromResourceEx(
            resource.as_ptr(),
            resource_size,
            1,
            0x0003_0000,
            32,
            32,
            LR_DEFAULTCOLOR,
        )
    };
    if icon.is_null() {
        bail!("无法加载内嵌托盘图标");
    }
    Ok(icon)
}

fn embedded_icon_resource(desired_size: u16) -> Result<&'static [u8]> {
    if ICON_BYTES.len() < 6
        || u16::from_le_bytes([ICON_BYTES[0], ICON_BYTES[1]]) != 0
        || u16::from_le_bytes([ICON_BYTES[2], ICON_BYTES[3]]) != 1
    {
        bail!("内嵌图标不是有效的 ICO 文件");
    }

    let count = u16::from_le_bytes([ICON_BYTES[4], ICON_BYTES[5]]) as usize;
    let directory_end = 6usize
        .checked_add(count.checked_mul(16).context("内嵌图标目录无效")?)
        .context("内嵌图标目录无效")?;
    if count == 0 || directory_end > ICON_BYTES.len() {
        bail!("内嵌图标目录无效");
    }

    let entry = (0..count)
        .map(|index| 6 + index * 16)
        .min_by_key(|offset| {
            let size = match ICON_BYTES[*offset] {
                0 => 256,
                value => u16::from(value),
            };
            size.abs_diff(desired_size)
        })
        .context("内嵌图标没有可用尺寸")?;
    let size = u32::from_le_bytes(
        ICON_BYTES[entry + 8..entry + 12]
            .try_into()
            .expect("ICO 目录项长度已验证"),
    ) as usize;
    let offset = u32::from_le_bytes(
        ICON_BYTES[entry + 12..entry + 16]
            .try_into()
            .expect("ICO 目录项长度已验证"),
    ) as usize;
    let end = offset.checked_add(size).context("内嵌图标数据无效")?;
    ICON_BYTES
        .get(offset..end)
        .context("内嵌图标数据超出文件范围")
}

unsafe fn create_window(config: &Config, icon: HICON) -> Result<(HWND, HWND, HWND, HFONT)> {
    let instance = unsafe { GetModuleHandleW(null()) };
    let class_name = wide("fy.PopupWindow");
    let window_class = WNDCLASSW {
        style: CS_DROPSHADOW,
        lpfnWndProc: Some(window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: icon,
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        hbrBackground: null_mut(),
        lpszMenuName: null(),
        lpszClassName: class_name.as_ptr(),
    };
    if unsafe { RegisterClassW(&window_class) } == 0 {
        bail!("无法注册浮窗类");
    }
    let title = wide("fy 翻译 — Ctrl+Enter 翻译，Esc 隐藏");
    let (window_x, window_y) = match (
        config.app.window_position,
        config.app.window_x,
        config.app.window_y,
    ) {
        (WindowPosition::Fixed, Some(x), Some(y)) => (x, y),
        _ => (CW_USEDEFAULT, CW_USEDEFAULT),
    };
    let extended_style = WS_EX_TOOLWINDOW
        | if config.app.always_on_top {
            WS_EX_TOPMOST
        } else {
            0
        };
    let hwnd = unsafe {
        CreateWindowExW(
            extended_style,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_POPUP | WS_CLIPCHILDREN,
            window_x,
            window_y,
            config.app.window_width,
            config.app.window_height,
            null_mut(),
            null_mut(),
            instance,
            null(),
        )
    };
    if hwnd.is_null() {
        bail!("无法创建浮窗");
    }
    let corner_preference: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND;
    let border_color: COLORREF = DWMWA_COLOR_NONE;
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &corner_preference as *const _ as *const _,
            size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR as u32,
            &border_color as *const _ as *const _,
            size_of::<COLORREF>() as u32,
        );
    }
    let font = unsafe {
        CreateFontW(
            -18,
            0,
            0,
            0,
            FW_NORMAL as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY as u32,
            DEFAULT_PITCH as u32,
            wide("Segoe UI").as_ptr(),
        )
    };
    if font.is_null() {
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd) };
        bail!("无法创建界面字体");
    }
    let edit_class = wide("EDIT");
    let input = unsafe { create_edit(hwnd, instance, &edit_class, INPUT_ID, false) };
    let output = unsafe { create_edit(hwnd, instance, &edit_class, OUTPUT_ID, true) };
    if input.is_null() || output.is_null() {
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
            DeleteObject(font);
        }
        bail!("无法创建输入框或翻译框");
    }
    unsafe {
        for (control, id) in [(input, INPUT_ID), (output, OUTPUT_ID)] {
            windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
                control,
                WM_SETFONT,
                font as usize,
                1,
            );
            SetWindowTheme(control, wide("DarkMode_Explorer").as_ptr(), null());
            SetWindowSubclass(control, Some(edit_proc), id as usize, 0);
        }
    }
    Ok((hwnd, input, output, font))
}

unsafe fn create_edit(
    parent: HWND,
    instance: *mut core::ffi::c_void,
    class: &[u16],
    id: i32,
    read_only: bool,
) -> HWND {
    let style = WS_CHILD
        | WS_VISIBLE
        | WS_TABSTOP
        | ES_MULTILINE as u32
        | ES_AUTOVSCROLL as u32
        | ES_WANTRETURN as u32
        | if read_only { ES_READONLY as u32 } else { 0 };
    unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            null(),
            style,
            0,
            0,
            0,
            0,
            parent,
            id as usize as HMENU,
            instance,
            null(),
        )
    }
}

#[derive(Clone, Copy)]
struct PopupLayout {
    input_card: RECT,
    output_card: RECT,
    input_edit: RECT,
    output_edit: RECT,
    splitter: RECT,
    pin_button: RECT,
    close_button: RECT,
}

fn popup_layout(hwnd: HWND) -> PopupLayout {
    let ratio = app_lock().map(|app| app.split_ratio).unwrap_or(0.4);
    popup_layout_without_lock(hwnd, ratio)
}

fn popup_layout_without_lock(hwnd: HWND, ratio: f32) -> PopupLayout {
    let mut client: RECT = unsafe { zeroed() };
    unsafe { GetClientRect(hwnd, &mut client) };
    let width = client.right.max(1);
    let height = client.bottom.max(1);
    let margin = 10;
    let gap = 12;
    let content_height = (height - HEADER_HEIGHT - margin - gap).max(210);
    let input_height = ((content_height as f32 * ratio) as i32).clamp(96, content_height - 110);
    let input_card = RECT {
        left: margin,
        top: HEADER_HEIGHT,
        right: width - margin,
        bottom: HEADER_HEIGHT + input_height,
    };
    let output_card = RECT {
        left: margin,
        top: input_card.bottom + gap,
        right: width - margin,
        bottom: height - margin,
    };
    PopupLayout {
        input_edit: inset_card(input_card),
        output_edit: inset_card(output_card),
        splitter: RECT {
            left: width / 2 - 24,
            top: input_card.bottom,
            right: width / 2 + 24,
            bottom: output_card.top,
        },
        input_card,
        output_card,
        pin_button: RECT {
            left: 10,
            top: 7,
            right: 42,
            bottom: 37,
        },
        close_button: RECT {
            left: width - 42,
            top: 7,
            right: width - 10,
            bottom: 37,
        },
    }
}

fn resize_controls(hwnd: HWND) {
    let input = unsafe { GetDlgItem(hwnd, INPUT_ID) };
    let output = unsafe { GetDlgItem(hwnd, OUTPUT_ID) };
    if input.is_null() || output.is_null() {
        return;
    }
    let layout = popup_layout(hwnd);
    unsafe {
        move_to_rect(input, layout.input_edit);
        move_to_rect(output, layout.output_edit);
        InvalidateRect(hwnd, null(), 0);
    }
}

fn update_split_ratio(hwnd: HWND, y: i32) {
    let mut client: RECT = unsafe { zeroed() };
    unsafe { GetClientRect(hwnd, &mut client) };
    let available = (client.bottom - HEADER_HEIGHT - 10 - 12).max(210);
    let ratio = ((y - HEADER_HEIGHT) as f32 / available as f32).clamp(0.2, 0.8);
    if let Some(mut app) = app_lock() {
        app.split_ratio = ratio;
        app.config.app.input_ratio = ratio;
    }
    resize_controls(hwnd);
}

fn inset_card(card: RECT) -> RECT {
    RECT {
        left: card.left + 12,
        top: card.top + 32,
        right: card.right - 12,
        bottom: card.bottom - 10,
    }
}

unsafe fn paint_popup(hwnd: HWND) {
    let mut paint: PAINTSTRUCT = unsafe { zeroed() };
    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
    if hdc.is_null() {
        return;
    }
    let mut client: RECT = unsafe { zeroed() };
    unsafe { GetClientRect(hwnd, &mut client) };
    let layout = popup_layout(hwnd);
    let (pinned, provider, font, translating, spinner_frame) = app_lock()
        .map(|app| {
            (
                app.pinned,
                app.config.active_provider.clone(),
                app.font,
                app.translating,
                app.spinner_frame,
            )
        })
        .unwrap_or((false, String::new(), null_mut(), false, 0));
    let brush = unsafe { GetStockObject(DC_BRUSH) };
    let pen = unsafe { GetStockObject(DC_PEN) };
    let old_brush = unsafe { SelectObject(hdc, brush) };
    let old_pen = unsafe { SelectObject(hdc, GetStockObject(NULL_PEN)) };
    unsafe {
        SetDCBrushColor(hdc, COLOR_WINDOW);
        FillRect(hdc, &client, brush as _);
        SetDCBrushColor(hdc, COLOR_CARD);
        for card in [layout.input_card, layout.output_card] {
            RoundRect(hdc, card.left, card.top, card.right, card.bottom, 14, 14);
        }
        SetDCBrushColor(hdc, if pinned { COLOR_ACCENT } else { COLOR_BUTTON });
        RoundRect(
            hdc,
            layout.pin_button.left,
            layout.pin_button.top,
            layout.pin_button.right,
            layout.pin_button.bottom,
            12,
            12,
        );
        SetDCBrushColor(hdc, COLOR_BUTTON);
        RoundRect(
            hdc,
            layout.close_button.left,
            layout.close_button.top,
            layout.close_button.right,
            layout.close_button.bottom,
            12,
            12,
        );

        SelectObject(
            hdc,
            if font.is_null() {
                GetStockObject(DEFAULT_GUI_FONT)
            } else {
                font
            },
        );
        SetBkMode(hdc, TRANSPARENT as i32);
        SetTextColor(hdc, COLOR_TEXT);
        draw_label(
            hdc,
            "fy",
            RECT {
                left: 52,
                top: 7,
                right: client.right - 52,
                bottom: 37,
            },
        );
        SetTextColor(hdc, COLOR_MUTED);
        draw_label(
            hdc,
            "原文 · Ctrl+Enter 翻译",
            RECT {
                left: layout.input_card.left + 12,
                top: layout.input_card.top + 4,
                right: layout.input_card.right - 12,
                bottom: layout.input_card.top + 30,
            },
        );
        let output_title = format!("翻译 · {provider}");
        draw_label(
            hdc,
            &output_title,
            RECT {
                left: layout.output_card.left + 12,
                top: layout.output_card.top + 4,
                right: layout.output_card.right - 12,
                bottom: layout.output_card.top + 30,
            },
        );

        SelectObject(hdc, pen);
        SetDCPenColor(hdc, COLOR_MUTED);
        let splitter_y = (layout.splitter.top + layout.splitter.bottom) / 2;
        MoveToEx(hdc, layout.splitter.left + 8, splitter_y, null_mut());
        LineTo(hdc, layout.splitter.right - 8, splitter_y);
        if translating {
            draw_spinner(
                hdc,
                (layout.output_card.left + 20 + output_title.encode_utf16().count() as i32 * 9)
                    .min(layout.output_card.right - 18),
                layout.output_card.top + 17,
                spinner_frame,
            );
        }
        SetDCPenColor(hdc, if pinned { COLOR_TEXT } else { COLOR_MUTED });
        draw_pin(hdc, layout.pin_button);
        SetDCPenColor(hdc, COLOR_TEXT);
        draw_close(hdc, layout.close_button);
        SelectObject(hdc, old_brush);
        SelectObject(hdc, old_pen);
        EndPaint(hwnd, &paint);
    }
}

unsafe fn draw_spinner(hdc: *mut core::ffi::c_void, center_x: i32, center_y: i32, frame: u8) {
    const POINTS: [(i32, i32); 8] = [
        (0, -6),
        (4, -4),
        (6, 0),
        (4, 4),
        (0, 6),
        (-4, 4),
        (-6, 0),
        (-4, -4),
    ];
    let (x, y) = POINTS[frame as usize % POINTS.len()];
    unsafe {
        SetDCPenColor(hdc, COLOR_ACCENT);
        MoveToEx(hdc, center_x + x / 2, center_y + y / 2, null_mut());
        LineTo(hdc, center_x + x, center_y + y);
    }
}

unsafe fn draw_label(hdc: *mut core::ffi::c_void, text: &str, mut rect: RECT) {
    let text = wide(text);
    unsafe {
        DrawTextW(
            hdc,
            text.as_ptr(),
            -1,
            &mut rect,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE,
        );
    }
}

unsafe fn draw_pin(hdc: *mut core::ffi::c_void, rect: RECT) {
    let center = (rect.left + rect.right) / 2;
    unsafe {
        MoveToEx(hdc, center - 6, rect.top + 9, null_mut());
        LineTo(hdc, center + 6, rect.top + 9);
        MoveToEx(hdc, center - 4, rect.top + 10, null_mut());
        LineTo(hdc, center - 2, rect.top + 17);
        LineTo(hdc, center + 2, rect.top + 17);
        LineTo(hdc, center + 4, rect.top + 10);
        MoveToEx(hdc, center - 6, rect.top + 18, null_mut());
        LineTo(hdc, center + 6, rect.top + 18);
        MoveToEx(hdc, center, rect.top + 18, null_mut());
        LineTo(hdc, center, rect.top + 25);
    }
}

unsafe fn draw_close(hdc: *mut core::ffi::c_void, rect: RECT) {
    unsafe {
        MoveToEx(hdc, rect.left + 10, rect.top + 9, null_mut());
        LineTo(hdc, rect.right - 10, rect.bottom - 9);
        MoveToEx(hdc, rect.right - 10, rect.top + 9, null_mut());
        LineTo(hdc, rect.left + 10, rect.bottom - 9);
    }
}

fn point_in_rect(point: POINT, rect: RECT) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

fn client_point(lparam: LPARAM) -> POINT {
    POINT {
        x: (lparam as u32 & 0xffff) as u16 as i16 as i32,
        y: ((lparam as u32 >> 16) & 0xffff) as u16 as i16 as i32,
    }
}

fn resize_hit_test(hwnd: HWND, lparam: LPARAM) -> Option<u32> {
    let mut window: RECT = unsafe { zeroed() };
    if unsafe { GetWindowRect(hwnd, &mut window) } == 0 {
        return None;
    }
    let point = POINT {
        x: (lparam as u32 & 0xffff) as u16 as i16 as i32,
        y: ((lparam as u32 >> 16) & 0xffff) as u16 as i16 as i32,
    };
    let border = 7;
    let corner = 14;
    let left = point.x < window.left + border;
    let right = point.x >= window.right - border;
    let top = point.y < window.top + border;
    let bottom = point.y >= window.bottom - border;
    let near_left = point.x < window.left + corner;
    let near_right = point.x >= window.right - corner;
    let near_top = point.y < window.top + corner;
    let near_bottom = point.y >= window.bottom - corner;
    match () {
        _ if near_top && near_left => Some(HTTOPLEFT),
        _ if near_top && near_right => Some(HTTOPRIGHT),
        _ if near_bottom && near_left => Some(HTBOTTOMLEFT),
        _ if near_bottom && near_right => Some(HTBOTTOMRIGHT),
        _ if left => Some(HTLEFT),
        _ if right => Some(HTRIGHT),
        _ if top => Some(HTTOP),
        _ if bottom => Some(HTBOTTOM),
        _ => None,
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCHITTEST => resize_hit_test(hwnd, lparam)
            .map(|hit| hit as LRESULT)
            .unwrap_or_else(|| unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }),
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            unsafe { paint_popup(hwnd) };
            0
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC => {
            let hdc = wparam as *mut core::ffi::c_void;
            unsafe {
                SetTextColor(hdc, COLOR_TEXT);
                SetBkColor(hdc, COLOR_CARD);
                SetDCBrushColor(hdc, COLOR_CARD);
                GetStockObject(DC_BRUSH) as LRESULT
            }
        }
        WM_GETMINMAXINFO => {
            let info = unsafe { &mut *(lparam as *mut MINMAXINFO) };
            info.ptMinTrackSize.x = MIN_WINDOW_WIDTH;
            info.ptMinTrackSize.y = MIN_WINDOW_HEIGHT;
            0
        }
        WM_SIZE => {
            resize_controls(hwnd);
            0
        }
        WM_ACTIVATE => {
            if wparam as u32 & 0xffff == WA_INACTIVE && app_lock().is_some_and(|app| !app.pinned) {
                unsafe { ShowWindow(hwnd, SW_HIDE) };
            }
            0
        }
        WM_TIMER if wparam == SPINNER_TIMER_ID => {
            if let Some(mut app) = app_lock()
                && app.translating
            {
                app.spinner_frame = app.spinner_frame.wrapping_add(1) % 8;
                let layout = popup_layout_without_lock(hwnd, app.split_ratio);
                let title = RECT {
                    left: layout.output_card.left,
                    top: layout.output_card.top,
                    right: layout.output_card.right,
                    bottom: layout.output_card.top + 32,
                };
                unsafe { InvalidateRect(hwnd, &title, 0) };
            }
            0
        }
        WM_LBUTTONDOWN => {
            let point = client_point(lparam);
            let layout = popup_layout(hwnd);
            if point_in_rect(point, layout.pin_button) {
                if let Some(mut app) = app_lock() {
                    let pinned = !app.pinned;
                    set_pinned(&mut app, pinned);
                }
            } else if point_in_rect(point, layout.close_button) {
                unsafe { ShowWindow(hwnd, SW_HIDE) };
            } else if point_in_rect(point, layout.splitter) {
                if let Some(mut app) = app_lock() {
                    app.dragging_splitter = true;
                }
                unsafe { SetCapture(hwnd) };
            } else if point.y < HEADER_HEIGHT {
                unsafe {
                    ReleaseCapture();
                    windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
                        hwnd,
                        WM_NCLBUTTONDOWN,
                        HTCAPTION as usize,
                        0,
                    );
                }
            }
            0
        }
        WM_MOUSEMOVE => {
            if app_lock().is_some_and(|app| app.dragging_splitter) {
                update_split_ratio(hwnd, client_point(lparam).y);
            }
            0
        }
        WM_LBUTTONUP => {
            let mut persist_error = None;
            if let Some(mut app) = app_lock()
                && app.dragging_splitter
            {
                app.dragging_splitter = false;
                let paths = app.paths.clone();
                let ratio = app.split_ratio;
                persist_error = app.config.set_input_ratio(&paths, ratio).err();
                unsafe { ReleaseCapture() };
            }
            if let Some(error) = persist_error {
                show_error(hwnd, &format!("保存上下分栏比例失败\n\n{error:#}"));
            }
            0
        }
        WM_HOTKEY => {
            begin_capture(hwnd);
            0
        }
        WM_CAPTURED => {
            let captured = unsafe { Box::from_raw(lparam as *mut Option<String>) };
            if let Some(text) = *captured {
                start_translation(text);
            } else {
                if let Some(mut app) = app_lock() {
                    app.request_id = app.request_id.wrapping_add(1);
                    app.translating = false;
                    app.streamed_text.clear();
                    unsafe { KillTimer(hwnd, SPINNER_TIMER_ID) };
                    set_text(app.input, "");
                    set_text(app.output, "");
                    set_title(app.hwnd, "fy 翻译 — 粘贴或输入后按 Ctrl+Enter");
                    set_pinned(&mut app, true);
                    show_popup(&app);
                }
            }
            0
        }
        WM_TRANSLATION_DELTA => {
            let delta = unsafe { Box::from_raw(lparam as *mut TranslationDelta) };
            let output = if let Some(mut app) = app_lock()
                && delta.id == app.request_id
            {
                if delta.from_cache {
                    app.streamed_text.clone_from(&delta.text);
                } else {
                    app.streamed_text.push_str(&delta.text);
                }
                Some(app.output)
            } else {
                None
            };
            if let Some(output) = output {
                if delta.from_cache {
                    set_text(output, &delta.text);
                    scroll_edit_to_top(output);
                } else {
                    append_text(output, &delta.text);
                }
            }
            0
        }
        WM_TRANSLATED => {
            let result = unsafe { Box::from_raw(lparam as *mut TranslationResult) };
            if let Some(mut app) = app_lock()
                && result.id == app.request_id
            {
                set_title(app.hwnd, "fy 翻译 — Ctrl+Enter 翻译，Esc 隐藏");
                if let Err(error) = &result.value {
                    if app.streamed_text.is_empty() {
                        app.streamed_text = format!("翻译失败\r\n\r\n{error}");
                    } else {
                        app.streamed_text
                            .push_str(&format!("\r\n\r\n翻译中断：{error}"));
                    }
                    set_text(app.output, &app.streamed_text);
                }
                app.translating = false;
                unsafe { KillTimer(hwnd, SPINNER_TIMER_ID) };
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            0
        }
        WM_MANUAL_TRANSLATE => {
            if let Some(app) = app_lock() {
                if app.translating {
                    return 0;
                }
                let text = window_text(app.input);
                drop(app);
                start_translation(text);
            }
            0
        }
        WM_TRAY => {
            match lparam as u32 {
                WM_LBUTTONDBLCLK => {
                    if let Some(app) = app_lock() {
                        show_popup(&app);
                    }
                }
                WM_RBUTTONUP => unsafe { show_tray_menu(hwnd) },
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            let command = wparam & 0xffff;
            if command >= MENU_PROVIDER_BASE {
                select_provider(command - MENU_PROVIDER_BASE);
                return 0;
            }
            match command {
                MENU_SHOW => {
                    if let Some(app) = app_lock() {
                        show_popup(&app);
                    }
                }
                MENU_RELOAD => reload_config(),
                MENU_FOLDER => open_config_folder(),
                MENU_EXIT => unsafe {
                    windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
                },
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            unsafe { ShowWindow(hwnd, SW_HIDE) };
            0
        }
        WM_DESTROY => {
            unsafe { KillTimer(hwnd, SPINNER_TIMER_ID) };
            if let Some(app) = app_lock() {
                unsafe {
                    UnregisterHotKey(hwnd, HOTKEY_ID);
                    Shell_NotifyIconW(NIM_DELETE, &app.tray);
                    DestroyIcon(app.icon);
                    DeleteObject(app.font);
                }
            }
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if message == WM_MOUSEMOVE {
        let point = client_point(lparam);
        let over_scrollbar = scrollbar_hit_test(hwnd, point);
        let dragging = app_lock().is_some_and(|app| app.scrollbar_drag == hwnd);
        set_scrollbar_hover(hwnd, over_scrollbar || dragging);
        let mut tracking = TRACKMOUSEEVENT {
            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: hwnd,
            dwHoverTime: 0,
        };
        unsafe { TrackMouseEvent(&mut tracking) };
        if dragging && wparam & MK_LBUTTON_STATE != 0 {
            scroll_edit_to(hwnd, point.y);
            return 0;
        }
    } else if message == WM_MOUSELEAVE {
        if !app_lock().is_some_and(|app| app.scrollbar_drag == hwnd) {
            set_scrollbar_hover(hwnd, false);
        }
    } else if message == WM_LBUTTONDOWN && scrollbar_hit_test(hwnd, client_point(lparam)) {
        if let Some(mut app) = app_lock() {
            app.scrollbar_drag = hwnd;
            app.scrollbar_hover = hwnd;
        }
        unsafe { SetCapture(hwnd) };
        scroll_edit_to(hwnd, client_point(lparam).y);
        return 0;
    } else if message == WM_LBUTTONUP && app_lock().is_some_and(|app| app.scrollbar_drag == hwnd) {
        if let Some(mut app) = app_lock() {
            app.scrollbar_drag = null_mut();
        }
        unsafe { ReleaseCapture() };
        set_scrollbar_hover(hwnd, scrollbar_hit_test(hwnd, client_point(lparam)));
        return 0;
    } else if message == WM_MOUSEWHEEL {
        let delta = (wparam >> 16) as u16 as i16 as i32;
        if delta != 0 {
            let steps = delta.unsigned_abs().div_ceil(120) as i32;
            let lines = -delta.signum() * steps * 3;
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
                    hwnd,
                    EM_LINESCROLL,
                    0,
                    lines as isize,
                );
            }
            repaint_scrollbar_gutter(hwnd);
        }
        return 0;
    }
    if message == WM_KEYDOWN {
        if wparam as u16 == VK_ESCAPE {
            let parent = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetParent(hwnd) };
            unsafe { ShowWindow(parent, SW_HIDE) };
            return 0;
        }
        if wparam as u16 == VK_RETURN && unsafe { GetKeyState(VK_CONTROL as i32) } < 0 {
            let parent = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetParent(hwnd) };
            unsafe { PostMessageW(parent, WM_MANUAL_TRANSLATE, 0, 0) };
            return 0;
        }
    }
    let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    if message == WM_PAINT {
        let visible = app_try_lock().is_some_and(|app| app.scrollbar_hover == hwnd);
        unsafe { paint_scrollbar_gutter(hwnd, visible) };
    }
    result
}

fn scrollbar_thumb(hwnd: HWND) -> Option<RECT> {
    let mut client: RECT = unsafe { zeroed() };
    unsafe { GetClientRect(hwnd, &mut client) };
    let height = client.bottom - client.top;
    let lines = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(hwnd, EM_GETLINECOUNT, 0, 0)
            as i32
    };
    let visible = visible_edit_lines(hwnd, height);
    if lines <= visible || height < 28 {
        return None;
    }
    let first = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
            hwnd,
            EM_GETFIRSTVISIBLELINE,
            0,
            0,
        ) as i32
    };
    let track_height = height - 8;
    let thumb_height = (track_height * visible / lines).clamp(24, track_height);
    let max_first = (lines - visible).max(1);
    let top = 4 + first.clamp(0, max_first) * (track_height - thumb_height) / max_first;
    Some(RECT {
        left: client.right - 7,
        top,
        right: client.right - 3,
        bottom: top + thumb_height,
    })
}

fn visible_edit_lines(hwnd: HWND, height: i32) -> i32 {
    let font = app_try_lock().map(|app| app.font).unwrap_or(null_mut());
    let hdc = unsafe { GetDC(hwnd) };
    if hdc.is_null() {
        return (height / 18).max(1);
    }
    let old_font = if font.is_null() {
        null_mut()
    } else {
        unsafe { SelectObject(hdc, font) }
    };
    let mut metrics: TEXTMETRICW = unsafe { zeroed() };
    let measured = unsafe { GetTextMetricsW(hdc, &mut metrics) } != 0;
    unsafe {
        if !old_font.is_null() {
            SelectObject(hdc, old_font);
        }
        ReleaseDC(hwnd, hdc);
    }
    let line_height = if measured { metrics.tmHeight } else { 18 }.max(1);
    (height / line_height).max(1)
}

fn scrollbar_hit_test(hwnd: HWND, point: POINT) -> bool {
    let mut client: RECT = unsafe { zeroed() };
    unsafe { GetClientRect(hwnd, &mut client) };
    point.x >= client.right - 12 && point.x < client.right && scrollbar_thumb(hwnd).is_some()
}

fn set_scrollbar_hover(hwnd: HWND, visible: bool) {
    let mut changed = false;
    if let Some(mut app) = app_lock() {
        let next = if visible { hwnd } else { null_mut() };
        if app.scrollbar_hover != next {
            app.scrollbar_hover = next;
            changed = true;
        }
    }
    if changed {
        repaint_scrollbar_gutter(hwnd);
    }
}

fn repaint_scrollbar_gutter(hwnd: HWND) {
    let visible = app_lock().is_some_and(|app| app.scrollbar_hover == hwnd);
    unsafe { paint_scrollbar_gutter(hwnd, visible) };
}

fn scroll_edit_to_top(hwnd: HWND) {
    let first = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
            hwnd,
            EM_GETFIRSTVISIBLELINE,
            0,
            0,
        ) as isize
    };
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(hwnd, EM_SETSEL, 0, 0);
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(hwnd, EM_LINESCROLL, 0, -first);
    }
}

fn scroll_edit_to(hwnd: HWND, y: i32) {
    let Some(thumb) = scrollbar_thumb(hwnd) else {
        return;
    };
    let mut client: RECT = unsafe { zeroed() };
    unsafe { GetClientRect(hwnd, &mut client) };
    let lines = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(hwnd, EM_GETLINECOUNT, 0, 0)
            as i32
    };
    let visible = visible_edit_lines(hwnd, client.bottom - client.top);
    let max_first = (lines - visible).max(1);
    let travel = (client.bottom - 8 - (thumb.bottom - thumb.top)).max(1);
    let target = ((y - 4 - (thumb.bottom - thumb.top) / 2).clamp(0, travel) * max_first) / travel;
    let current = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
            hwnd,
            EM_GETFIRSTVISIBLELINE,
            0,
            0,
        ) as i32
    };
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
            hwnd,
            EM_LINESCROLL,
            0,
            (target - current) as isize,
        );
    }
    repaint_scrollbar_gutter(hwnd);
}

unsafe fn paint_scrollbar_gutter(hwnd: HWND, visible: bool) {
    let mut gutter: RECT = unsafe { zeroed() };
    unsafe { GetClientRect(hwnd, &mut gutter) };
    gutter.left = (gutter.right - 12).max(gutter.left);
    let hdc = unsafe { GetDC(hwnd) };
    if hdc.is_null() {
        return;
    }
    unsafe {
        let old_brush = SelectObject(hdc, GetStockObject(DC_BRUSH));
        let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
        SetDCBrushColor(hdc, COLOR_CARD);
        FillRect(hdc, &gutter, GetStockObject(DC_BRUSH));
        if visible && let Some(thumb) = scrollbar_thumb(hwnd) {
            SetDCBrushColor(hdc, COLOR_MUTED);
            RoundRect(hdc, thumb.left, thumb.top, thumb.right, thumb.bottom, 4, 4);
        }
        SelectObject(hdc, old_brush);
        SelectObject(hdc, old_pen);
        ReleaseDC(hwnd, hdc);
    }
}

fn begin_capture(hwnd: HWND) {
    let hwnd_value = hwnd as usize;
    thread::spawn(move || {
        let hwnd = hwnd_value as HWND;
        let captured = selection::get_text();
        let captured = (!captured.is_empty()).then_some(captured);
        post_captured(hwnd, captured);
    });
}

fn post_captured(hwnd: HWND, captured: Option<String>) {
    unsafe {
        PostMessageW(
            hwnd,
            WM_CAPTURED,
            0,
            Box::into_raw(Box::new(captured)) as isize,
        )
    };
}

fn start_translation(source: String) {
    let (hwnd, config, cache, id) = {
        let Some(mut app) = app_lock() else { return };
        if source.trim().is_empty() {
            set_text(app.output, "请输入需要翻译的文本。");
            return;
        }
        app.request_id = app.request_id.wrapping_add(1);
        app.translating = true;
        app.spinner_frame = 0;
        app.streamed_text.clear();
        set_text(app.input, &source);
        set_text(app.output, "");
        set_title(app.hwnd, "fy 翻译 — 正在翻译…");
        unsafe { SetTimer(app.hwnd, SPINNER_TIMER_ID, 90, None) };
        unsafe { InvalidateRect(app.hwnd, null(), 0) };
        show_popup(&app);
        (
            app.hwnd as usize,
            app.config.clone(),
            app.paths.cache.clone(),
            app.request_id,
        )
    };
    thread::spawn(move || {
        let hwnd = hwnd as HWND;
        let value = translator::translate(&config, &cache, &source, |text, from_cache| {
            let delta = TranslationDelta {
                id,
                text: text.to_owned(),
                from_cache,
            };
            unsafe {
                PostMessageW(
                    hwnd,
                    WM_TRANSLATION_DELTA,
                    0,
                    Box::into_raw(Box::new(delta)) as isize,
                )
            };
        })
        .map(|_| ())
        .map_err(|e| format!("{e:#}"));
        let result = TranslationResult { id, value };
        unsafe {
            PostMessageW(
                hwnd,
                WM_TRANSLATED,
                0,
                Box::into_raw(Box::new(result)) as isize,
            )
        };
    });
}

struct TranslationDelta {
    id: u64,
    text: String,
    from_cache: bool,
}

struct TranslationResult {
    id: u64,
    value: std::result::Result<(), String>,
}

fn register_hotkey(app: &mut App) -> Result<()> {
    let (modifiers, key) = parse_hotkey(&app.config.app.hotkey)?;
    unsafe { UnregisterHotKey(app.hwnd, HOTKEY_ID) };
    if unsafe { RegisterHotKey(app.hwnd, HOTKEY_ID, modifiers | MOD_NOREPEAT, key as u32) } == 0 {
        bail!(
            "无法注册快捷键 {}，它可能已被其他程序占用",
            app.config.app.hotkey
        );
    }
    Ok(())
}

fn parse_hotkey(value: &str) -> Result<(u32, u16)> {
    let mut modifiers = 0;
    let mut key = None;
    for part in value.split('+').map(str::trim) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => modifiers |= MOD_CONTROL,
            "alt" => modifiers |= MOD_ALT,
            "shift" => modifiers |= MOD_SHIFT,
            "win" | "windows" => modifiers |= MOD_WIN,
            token if token.len() == 1 => {
                let byte = token.as_bytes()[0].to_ascii_uppercase();
                if byte.is_ascii_alphanumeric() {
                    key = Some(byte as u16);
                } else {
                    bail!("不支持的快捷键: {value}");
                }
            }
            _ => bail!("不支持的快捷键: {value}"),
        }
    }
    Ok((modifiers, key.context("快捷键缺少主按键")?))
}

fn select_provider(index: usize) {
    let Some(mut app) = app_lock() else { return };
    let Some(name) = app
        .config
        .providers
        .get(index)
        .map(|provider| provider.name.clone())
    else {
        return;
    };
    if name == app.config.active_provider {
        return;
    }
    let paths = app.paths.clone();
    if let Err(error) = app.config.select_provider(&paths, &name) {
        show_error(app.hwnd, &format!("切换服务商失败\n\n{error:#}"));
    } else {
        unsafe { InvalidateRect(app.hwnd, null(), 0) };
    }
}

fn reload_config() {
    match Config::load_or_create() {
        Ok((config, paths)) => {
            if let Some(mut app) = app_lock() {
                let old = app.config.clone();
                app.config = config;
                app.paths = paths;
                if let Err(error) =
                    register_hotkey(&mut app).and_then(|_| apply_autostart(&app.config))
                {
                    app.config = old;
                    app.split_ratio = app.config.app.input_ratio;
                    let _ = register_hotkey(&mut app);
                    show_error(app.hwnd, &format!("重新加载配置失败\n\n{error:#}"));
                } else {
                    app.pinned = app.config.app.always_on_top;
                    app.split_ratio = app.config.app.input_ratio;
                    apply_window_config(&app);
                    unsafe { PostMessageW(app.hwnd, WM_SIZE, 0, 0) };
                    set_text(app.output, "配置已重新加载。");
                    unsafe { InvalidateRect(app.hwnd, null(), 0) };
                    show_popup(&app);
                }
            }
        }
        Err(error) => show_error(null_mut(), &format!("重新加载配置失败\n\n{error:#}")),
    }
}

fn apply_autostart(config: &Config) -> Result<()> {
    let key_path = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let value_name = wide("fy");
    unsafe {
        let mut key = null_mut();
        let status = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            key_path.as_ptr(),
            0,
            null_mut(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            null(),
            &mut key,
            null_mut(),
        );
        if status != 0 {
            bail!("无法打开开机启动注册表项，错误码 {status}");
        }
        let result = if config.app.autostart {
            let exe = std::env::current_exe()?;
            let command = wide(&format!("\"{}\"", exe.display()));
            RegSetValueExW(
                key,
                value_name.as_ptr(),
                0,
                REG_SZ,
                command.as_ptr() as *const u8,
                (command.len() * size_of::<u16>()) as u32,
            )
        } else {
            let status = RegDeleteValueW(key, value_name.as_ptr());
            if status == 2 { 0 } else { status }
        };
        RegCloseKey(key);
        if result != 0 {
            bail!("无法更新开机启动设置，错误码 {result}");
        }
    }
    Ok(())
}

unsafe fn add_tray_icon(hwnd: HWND, icon: HICON) -> Result<NOTIFYICONDATAW> {
    let mut tray: NOTIFYICONDATAW = unsafe { zeroed() };
    tray.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    tray.hWnd = hwnd;
    tray.uID = TRAY_ID;
    tray.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    tray.uCallbackMessage = WM_TRAY;
    tray.hIcon = icon;
    copy_wide(&mut tray.szTip, "fy 划词翻译");
    if unsafe { Shell_NotifyIconW(NIM_ADD, &tray) } == 0 {
        bail!("无法创建托盘图标");
    }
    Ok(tray)
}

unsafe fn show_tray_menu(hwnd: HWND) {
    let menu = unsafe { CreatePopupMenu() };
    if menu.is_null() {
        return;
    }
    let provider_menu = unsafe { CreatePopupMenu() };
    if provider_menu.is_null() {
        unsafe { DestroyMenu(menu) };
        return;
    }
    let providers = app_lock()
        .map(|app| {
            app.config
                .providers
                .iter()
                .map(|provider| {
                    (
                        provider.name.clone(),
                        provider.name == app.config.active_provider,
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    unsafe {
        AppendMenuW(menu, MF_STRING, MENU_SHOW, wide("显示浮窗").as_ptr());
        for (index, (name, active)) in providers.iter().enumerate() {
            let flags = MF_STRING | if *active { MF_CHECKED } else { 0 };
            AppendMenuW(
                provider_menu,
                flags,
                MENU_PROVIDER_BASE + index,
                wide(name).as_ptr(),
            );
        }
        AppendMenuW(
            menu,
            MF_POPUP,
            provider_menu as usize,
            wide("切换服务商").as_ptr(),
        );
        AppendMenuW(menu, MF_STRING, MENU_RELOAD, wide("重新加载配置").as_ptr());
        AppendMenuW(menu, MF_STRING, MENU_FOLDER, wide("打开配置目录").as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, MF_STRING, MENU_EXIT, wide("退出").as_ptr());
        let mut point: POINT = zeroed();
        GetCursorPos(&mut point);
        SetForegroundWindow(hwnd);
        TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            0,
            hwnd,
            null(),
        );
        DestroyMenu(menu);
    }
}

fn open_config_folder() {
    if let Some(app) = app_lock() {
        let folder = wide(&app.paths.root.to_string_lossy());
        unsafe {
            ShellExecuteW(
                app.hwnd,
                wide("open").as_ptr(),
                folder.as_ptr(),
                null(),
                null(),
                SW_SHOWNORMAL,
            )
        };
    }
}

fn show_popup(app: &App) {
    unsafe {
        if IsWindowVisible(app.hwnd) == 0 && app.config.app.window_position == WindowPosition::Auto
        {
            position_window_near_mouse(app.hwnd);
        }
        SetWindowPos(
            app.hwnd,
            if app.pinned {
                HWND_TOPMOST
            } else {
                HWND_NOTOPMOST
            },
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE,
        );
        ShowWindow(app.hwnd, SW_SHOW);
        SetForegroundWindow(app.hwnd);
        SetFocus(app.input);
    }
}

fn set_pinned(app: &mut App, pinned: bool) {
    app.pinned = pinned;
    unsafe {
        SetWindowPos(
            app.hwnd,
            if pinned { HWND_TOPMOST } else { HWND_NOTOPMOST },
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE,
        );
        InvalidateRect(app.hwnd, null(), 0);
    }
}

fn apply_window_config(app: &App) {
    let (x, y, position_flags) = match (
        app.config.app.window_position,
        app.config.app.window_x,
        app.config.app.window_y,
    ) {
        (WindowPosition::Fixed, Some(x), Some(y)) => (x, y, 0),
        _ => (0, 0, SWP_NOMOVE),
    };
    unsafe {
        SetWindowPos(
            app.hwnd,
            if app.pinned {
                HWND_TOPMOST
            } else {
                HWND_NOTOPMOST
            },
            x,
            y,
            app.config.app.window_width,
            app.config.app.window_height,
            position_flags,
        );
    }
}

unsafe fn position_window_near_mouse(hwnd: HWND) {
    let mut cursor: POINT = unsafe { zeroed() };
    let mut window: RECT = unsafe { zeroed() };
    if unsafe { GetCursorPos(&mut cursor) } == 0 || unsafe { GetWindowRect(hwnd, &mut window) } == 0
    {
        return;
    }
    let monitor = unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) };
    let mut monitor_info: MONITORINFO = unsafe { zeroed() };
    monitor_info.cbSize = size_of::<MONITORINFO>() as u32;
    if monitor.is_null() || unsafe { GetMonitorInfoW(monitor, &mut monitor_info) } == 0 {
        return;
    }

    let width = window.right - window.left;
    let height = window.bottom - window.top;
    let work = monitor_info.rcWork;
    let mut x = cursor.x + 16;
    let mut y = cursor.y + 20;
    if x + width > work.right {
        x = cursor.x - width - 16;
    }
    if y + height > work.bottom {
        y = cursor.y - height - 16;
    }
    x = x.clamp(work.left, (work.right - width).max(work.left));
    y = y.clamp(work.top, (work.bottom - height).max(work.top));
    unsafe { SetWindowPos(hwnd, null_mut(), x, y, 0, 0, SWP_NOSIZE) };
}

unsafe fn move_to_rect(hwnd: HWND, rect: RECT) {
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::MoveWindow(
            hwnd,
            rect.left,
            rect.top,
            (rect.right - rect.left).max(1),
            (rect.bottom - rect.top).max(1),
            1,
        );
    }
}

fn set_text(hwnd: HWND, text: &str) {
    let windows_text = windows_text(text);
    unsafe { SetWindowTextW(hwnd, wide(&windows_text).as_ptr()) };
}

fn append_text(hwnd: HWND, text: &str) {
    let windows_text = windows_text(text);
    let end = unsafe { GetWindowTextLengthW(hwnd) } as usize;
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
            hwnd,
            EM_SETSEL,
            end,
            end as isize,
        );
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
            hwnd,
            EM_REPLACESEL,
            0,
            wide(&windows_text).as_ptr() as isize,
        );
    }
}

fn windows_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n")
}

fn set_title(hwnd: HWND, title: &str) {
    unsafe { SetWindowTextW(hwnd, wide(title).as_ptr()) };
}

fn window_text(hwnd: HWND) -> String {
    unsafe {
        let length = GetWindowTextLengthW(hwnd);
        let mut buffer = vec![0u16; length as usize + 1];
        GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        String::from_utf16_lossy(&buffer[..length as usize])
    }
}

fn show_error(hwnd: HWND, message: &str) {
    unsafe {
        MessageBoxW(
            hwnd,
            wide(message).as_ptr(),
            wide("fy").as_ptr(),
            MB_OK | MB_ICONERROR,
        )
    };
}

fn app_lock() -> Option<std::sync::MutexGuard<'static, App>> {
    APP.get().map(|app| app.lock().unwrap())
}

fn app_try_lock() -> Option<std::sync::MutexGuard<'static, App>> {
    APP.get().and_then(|app| app.try_lock().ok())
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn copy_wide<const N: usize>(target: &mut [u16; N], value: &str) {
    let encoded = wide(value);
    let length = encoded.len().min(N);
    target[..length].copy_from_slice(&encoded[..length]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_hotkey() {
        let (modifiers, key) = parse_hotkey("Alt+X").unwrap();
        assert_eq!(key, b'X' as u16);
        assert_eq!(modifiers & MOD_CONTROL, 0);
        assert_ne!(modifiers & MOD_ALT, 0);
    }

    #[test]
    fn rejects_unknown_hotkey() {
        assert!(parse_hotkey("Ctrl+F12").is_err());
    }

    #[test]
    fn loads_the_embedded_tray_icon_resource() {
        assert!(!embedded_icon_resource(32).unwrap().is_empty());
    }
}
