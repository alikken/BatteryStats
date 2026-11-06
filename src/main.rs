#![windows_subsystem = "windows"]
//! BatteryStats — мощность батареи ноутбука в трее Windows (аналог Stats для macOS).

mod battery;
mod i18n;
mod icon;
mod popup;
mod procs;
mod report;
mod usage;
mod util;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::Graphics::GdiPlus::{GdiplusShutdown, GdiplusStartup, GdiplusStartupInput};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use battery::{Reader, Snapshot};
use i18n::{t, Lang};
use procs::{ProcStats, Sample, Sampler, SYSTEM_SHARE};
use usage::Usage;
use util::*;

const WM_TRAY: u32 = WM_APP + 1;
const WM_DATA: u32 = WM_APP + 2;
const WM_PROCS: u32 = WM_APP + 3;
const TRAY_ID: u32 = 1;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const AUTHOR: &str = "alikken";

const CMD_SHOW: usize = 100;
const CMD_INT_1: usize = 101;
const CMD_INT_2: usize = 102;
const CMD_INT_5: usize = 103;
const CMD_MODE_W: usize = 110;
const CMD_MODE_P: usize = 111;
const CMD_AUTOSTART: usize = 120;
const CMD_LANG_AUTO: usize = 130; // 130..=133: авто, en, ru, pl
const CMD_OPEN_FOLDER: usize = 140;
const CMD_EXIT: usize = 199;
const CMD_REPORT_BASE: usize = 300;

static LATEST: Mutex<Option<Snapshot>> = Mutex::new(None);
static INTERVAL_MS: AtomicU32 = AtomicU32::new(1000);
static WAKE_M: Mutex<bool> = Mutex::new(false);
static WAKE_CV: Condvar = Condvar::new();
static POPUP_HIDDEN_AT: AtomicU32 = AtomicU32::new(0);
static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
static POPUP_HWND: AtomicIsize = AtomicIsize::new(0);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static DEMO: AtomicBool = AtomicBool::new(false);
static PROCS: Mutex<Option<ProcStats>> = Mutex::new(None);
static POPUP_OPEN: AtomicBool = AtomicBool::new(false);
static PROC_M: Mutex<bool> = Mutex::new(false);
static PROC_CV: Condvar = Condvar::new();
static PROC_ROWS: AtomicI32 = AtomicI32::new(6);
/// Энергия разряда, ещё не разнесённая по программам: (Вт·ч, секунд).
static PENDING: Mutex<(f64, f64)> = Mutex::new((0.0, 0.0));
static DISCHARGING: AtomicBool = AtomicBool::new(false);
static USAGE: Mutex<Option<Usage>> = Mutex::new(None);
/// 0 — как в Windows, 1 — en, 2 — ru, 3 — pl.
static LANG_SETTING: AtomicU32 = AtomicU32::new(0);

struct App {
    snap: Snapshot,
    history: VecDeque<f32>,
    icon: HICON,
    icon_mode: u32, // 0 — ватты, 1 — проценты
    procs: Option<ProcStats>,
}

thread_local! {
    static APP: RefCell<App> = RefCell::new(App {
        snap: Snapshot::default(),
        history: VecDeque::with_capacity(popup::HISTORY),
        icon: null_mut(),
        icon_mode: 0,
        procs: None,
    });
}

fn main_hwnd() -> HWND {
    MAIN_HWND.load(Ordering::Relaxed) as HWND
}
fn popup_hwnd() -> HWND {
    POPUP_HWND.load(Ordering::Relaxed) as HWND
}

fn set_popup_open(open: bool) {
    POPUP_OPEN.store(open, Ordering::Relaxed);
    wake_procs();
}

fn wake_procs() {
    if let Ok(mut g) = PROC_M.lock() {
        *g = true;
        PROC_CV.notify_one();
    }
}

fn apply_lang(setting: u32) {
    LANG_SETTING.store(setting, Ordering::Relaxed);
    i18n::set_lang(match setting {
        1 => Lang::En,
        2 => Lang::Ru,
        3 => Lang::Pl,
        _ => i18n::detect(),
    });
}

fn save_usage() {
    if let Ok(mut g) = USAGE.lock() {
        if let Some(u) = g.as_mut() {
            u.save(&data_dir(), &today());
        }
    }
}

/// Разнести энергию разряда за интервал по программам пропорционально нагрузке.
fn attribute(sampler: &mut Sampler, sample: &Sample, wh: f64, secs: f64) {
    if wh <= 0.0 && secs <= 0.0 {
        return;
    }
    let mut parts: Vec<(String, String, f64, f64)> = Vec::new();
    match sample {
        Sample::Data(groups) => {
            let total: f32 = groups.iter().map(|g| g.impact()).sum();
            let denom = (total + SYSTEM_SHARE) as f64;
            for g in groups {
                let e = wh * g.impact() as f64 / denom;
                let cpu_s = g.cpu as f64 / 100.0 * sampler.ncpu as f64 * secs;
                if e < 1e-6 && cpu_s < 0.5 {
                    continue;
                }
                let name = sampler.display_name(&g.key, g.pid);
                parts.push((g.key.clone(), name, e, cpu_s));
            }
            parts.push((usage::OTHER.into(), String::new(), wh * SYSTEM_SHARE as f64 / denom, 0.0));
        }
        _ => parts.push((usage::OTHER.into(), String::new(), wh, 0.0)),
    }
    if let Ok(mut g) = USAGE.lock() {
        if let Some(u) = g.as_mut() {
            u.add(&today(), wh, secs, &parts);
        }
    }
}

unsafe fn shell_open(path: &str) {
    ShellExecuteW(null_mut(), w("open").as_ptr(), w(path).as_ptr(), null(), null(), SW_SHOWNORMAL);
}

unsafe fn open_report(ym: &str) {
    save_usage();
    let full = APP.with(|a| a.try_borrow().ok().and_then(|a| a.snap.full_wh));
    let cur = today()[..7].to_string();
    if let Ok(p) = report::write_all(&data_dir(), ym, &cur, full) {
        shell_open(&p.to_string_lossy());
    }
}

fn wake_poller() {
    if let Ok(mut g) = WAKE_M.lock() {
        *g = true;
        WAKE_CV.notify_one();
    }
}

fn copy_w(dst: &mut [u16], s: &str) {
    let mut i = 0;
    for c in s.encode_utf16() {
        if i + 1 >= dst.len() {
            break;
        }
        dst[i] = c;
        i += 1;
    }
    dst[i] = 0;
}

fn tray_text(s: &Snapshot, mode: u32) -> String {
    if mode == 1 {
        return match s.percent {
            Some(p) => format!("{:.0}", p),
            None => "—".into(),
        };
    }
    match s.power_w {
        Some(p) => {
            let a = p.abs();
            if a < 9.95 {
                f1(a)
            } else {
                format!("{:.0}", a)
            }
        }
        None => match s.percent {
            Some(p) if s.present => format!("{:.0}", p),
            _ => "—".into(),
        },
    }
}

fn tray_color(s: &Snapshot, mode: u32) -> u32 {
    let base = if taskbar_is_dark() { 0xFFFFFF } else { 0x111111 };
    if s.charging {
        return if taskbar_is_dark() { 0x4CD98A } else { 0x1E9E55 };
    }
    if s.discharging {
        let p = s.percent.unwrap_or(100.0);
        if mode == 1 && p < 10.0 {
            return 0xFF5A52;
        }
        if mode == 1 && p < 20.0 {
            return 0xFFA43A;
        }
        return base;
    }
    if s.on_ac && s.present {
        return if taskbar_is_dark() { 0x7DB8FF } else { 0x2F7CF6 };
    }
    base
}

fn tooltip(s: &Snapshot) -> String {
    let tr = t();
    if !s.present {
        return tr.tip_nobattery.into();
    }
    let mut t = String::from("BatteryStats");
    if let Some(p) = s.percent {
        t += &format!(" — {:.0}%, {}", p, popup::status_text(s).to_lowercase());
    }
    let mut line = Vec::new();
    if let Some(p) = s.power_w {
        line.push(format!("{} {}", f1(p.abs()), tr.u_w));
    }
    if let Some(v) = s.voltage_v {
        line.push(format!("{} {}", f2(v), tr.u_v));
    }
    if let Some(a) = s.current_a {
        line.push(format!("{} {}", f2(a), tr.u_a));
    }
    if !line.is_empty() {
        t += "\n";
        t += &line.join(" · ");
    }
    if s.time_left_min.is_some() && (s.charging || s.discharging) {
        t += "\n";
        t += &popup::subtitle(s);
    }
    t
}

unsafe fn icon_size() -> i32 {
    let dpi = GetDpiForSystem().max(96);
    GetSystemMetricsForDpi(SM_CXSMICON, dpi).max(16)
}

unsafe fn notify_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid: NOTIFYICONDATAW = zeroed();
    nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_ID;
    nid
}

unsafe fn tray_add(hwnd: HWND) {
    let (text, color, tip, mode) = APP.with(|a| {
        let a = a.borrow();
        (tray_text(&a.snap, a.icon_mode), tray_color(&a.snap, a.icon_mode), tooltip(&a.snap), a.icon_mode)
    });
    let _ = mode;
    let ic = icon::text_icon(&text, color, icon_size());
    let mut nid = notify_data(hwnd);
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = ic;
    copy_w(&mut nid.szTip, &tip);
    Shell_NotifyIconW(NIM_ADD, &nid);
    let old = APP.with(|a| std::mem::replace(&mut a.borrow_mut().icon, ic));
    if !old.is_null() {
        DestroyIcon(old);
    }
}

unsafe fn tray_update(hwnd: HWND) {
    let Some((text, color, tip)) = APP.with(|a| {
        a.try_borrow().ok().map(|a| (tray_text(&a.snap, a.icon_mode), tray_color(&a.snap, a.icon_mode), tooltip(&a.snap)))
    }) else {
        return;
    };
    let ic = icon::text_icon(&text, color, icon_size());
    let mut nid = notify_data(hwnd);
    nid.uFlags = NIF_ICON | NIF_TIP;
    nid.hIcon = ic;
    copy_w(&mut nid.szTip, &tip);
    if Shell_NotifyIconW(NIM_MODIFY, &nid) == 0 {
        // Значок пропал (например, перезапуск проводника) — добавить заново.
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY;
        Shell_NotifyIconW(NIM_ADD, &nid);
    }
    let old = APP.with(|a| match a.try_borrow_mut() {
        Ok(mut a) => std::mem::replace(&mut a.icon, ic),
        Err(_) => null_mut(),
    });
    if !old.is_null() {
        DestroyIcon(old);
    }
}

unsafe fn tray_remove(hwnd: HWND) {
    let nid = notify_data(hwnd);
    Shell_NotifyIconW(NIM_DELETE, &nid);
}

// ---------------- Всплывающее окно ----------------

unsafe fn popup_size_for(dpi: u32) -> (i32, i32) {
    let rows = PROC_ROWS.load(Ordering::Relaxed);
    let h = APP.with(|a| a.try_borrow().map(|a| popup::layout(&a.snap, rows).height).unwrap_or(640));
    let k = dpi as f32 / 96.0;
    ((popup::WIDTH as f32 * k).round() as i32, (h as f32 * k).round() as i32)
}

unsafe fn show_popup() {
    let popup = popup_hwnd();
    let main = main_hwnd();

    // Где значок?
    let mut anchor: RECT = zeroed();
    let mut id: NOTIFYICONIDENTIFIER = zeroed();
    id.cbSize = size_of::<NOTIFYICONIDENTIFIER>() as u32;
    id.hWnd = main;
    id.uID = TRAY_ID;
    let mut cur: POINT = zeroed();
    GetCursorPos(&mut cur);
    if Shell_NotifyIconGetRect(&id, &mut anchor) != 0 || anchor.right <= anchor.left {
        anchor = RECT { left: cur.x, top: cur.y, right: cur.x + 1, bottom: cur.y + 1 };
    }
    let center = POINT { x: (anchor.left + anchor.right) / 2, y: (anchor.top + anchor.bottom) / 2 };
    let mon = MonitorFromPoint(center, MONITOR_DEFAULTTONEAREST);
    let mut mi: MONITORINFO = zeroed();
    mi.cbSize = size_of::<MONITORINFO>() as u32;
    GetMonitorInfoW(mon, &mut mi);
    let work = mi.rcWork;
    let (mut dx, mut dy) = (96u32, 96u32);
    if GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) != 0 {
        dx = GetDpiForSystem();
    }
    let gap = (12.0 * dx as f32 / 96.0) as i32;
    // Сколько строк процессов влезает по высоте экрана.
    let k = dx as f32 / 96.0;
    let avail = ((work.bottom - work.top - 2 * gap) as f32 / k) as i32;
    let base = APP.with(|a| a.try_borrow().map(|a| popup::layout(&a.snap, 0).height).unwrap_or(460));
    PROC_ROWS.store(((avail - base) / popup::PROC_ROW_H).clamp(2, 6), Ordering::Relaxed);
    let (wd, ht) = popup_size_for(dx);

    let mut x = center.x - wd / 2;
    let mut y = if center.y > (work.top + work.bottom) / 2 { work.bottom - ht - gap } else { work.top + gap };
    if center.x < work.left {
        x = work.left + gap; // панель задач слева
    } else if center.x > work.right {
        x = work.right - wd - gap; // справа
    }
    x = x.clamp(work.left + gap, (work.right - wd - gap).max(work.left));
    y = y.clamp(work.top + gap.min(4), (work.bottom - ht - gap.min(4)).max(work.top));

    SetWindowPos(popup, HWND_TOPMOST, x, y, wd, ht, SWP_NOACTIVATE);
    // Если окно оказалось на мониторе с другим DPI — подогнать размер.
    let real = GetDpiForWindow(popup);
    if real != dx && real != 0 {
        let (wd2, ht2) = popup_size_for(real);
        SetWindowPos(popup, HWND_TOPMOST, x, y, wd2, ht2, SWP_NOACTIVATE);
    }
    set_popup_open(true);
    ShowWindow(popup, SW_SHOW);
    SetForegroundWindow(popup);
    InvalidateRect(popup, null(), 0);
}

unsafe fn hide_popup() {
    let popup = popup_hwnd();
    if IsWindowVisible(popup) != 0 {
        ShowWindow(popup, SW_HIDE);
        POPUP_HIDDEN_AT.store(GetTickCount(), Ordering::Relaxed);
    }
    set_popup_open(false);
}

unsafe fn toggle_popup() {
    let popup = popup_hwnd();
    if IsWindowVisible(popup) != 0 {
        hide_popup();
    } else if GetTickCount().wrapping_sub(POPUP_HIDDEN_AT.load(Ordering::Relaxed)) > 400 {
        show_popup();
    }
}

unsafe fn resize_popup_if_needed() {
    let popup = popup_hwnd();
    if IsWindowVisible(popup) == 0 {
        return;
    }
    let dpi = GetDpiForWindow(popup).max(96);
    let (wd, ht) = popup_size_for(dpi);
    let mut r: RECT = zeroed();
    GetWindowRect(popup, &mut r);
    if r.bottom - r.top != ht || r.right - r.left != wd {
        // Держим нижний край на месте.
        SetWindowPos(popup, HWND_TOPMOST, r.left, r.bottom - ht, wd, ht, SWP_NOACTIVATE);
    }
    InvalidateRect(popup, null(), 0);
}

unsafe extern "system" fn popup_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ACTIVATE => {
            if (wp & 0xFFFF) as u32 == WA_INACTIVE {
                hide_popup();
            }
            0
        }
        WM_KEYDOWN if wp == 0x1B => {
            hide_popup();
            0
        }
        WM_ERASEBKGND => 1,
        WM_LBUTTONUP => {
            let x = (lp & 0xFFFF) as i16 as i32;
            let y = ((lp >> 16) & 0xFFFF) as i16 as i32;
            if popup::link_hit(x, y) {
                hide_popup();
                open_report(&today()[..7]);
            }
            0
        }
        WM_SETCURSOR => {
            let mut pt: POINT = zeroed();
            GetCursorPos(&mut pt);
            ScreenToClient(hwnd, &mut pt);
            let cur = if popup::link_hit(pt.x, pt.y) { IDC_HAND } else { IDC_ARROW };
            SetCursor(LoadCursorW(null_mut(), cur));
            1
        }
        WM_DPICHANGED => {
            let r = &*(lp as *const RECT);
            let (wd, ht) = popup_size_for((wp & 0xFFFF) as u32);
            SetWindowPos(hwnd, null_mut(), r.left, r.top, wd, ht, SWP_NOZORDER | SWP_NOACTIVATE);
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rc: RECT = zeroed();
            GetClientRect(hwnd, &mut rc);
            let (wd, ht) = (rc.right - rc.left, rc.bottom - rc.top);
            if wd > 0 && ht > 0 {
                let mem = CreateCompatibleDC(hdc);
                let bmp = CreateCompatibleBitmap(hdc, wd, ht);
                let old = SelectObject(mem, bmp);
                let dpi = GetDpiForWindow(hwnd).max(96);
                let dark = apps_are_dark();
                let interval = INTERVAL_MS.load(Ordering::Relaxed);
                let painted = APP.with(|a| match a.try_borrow() {
                    Ok(a) => {
                        let view = popup::View {
                            snap: &a.snap,
                            history: &a.history,
                            interval_ms: interval,
                            procs: a.procs.as_ref(),
                            proc_rows: PROC_ROWS.load(Ordering::Relaxed),
                        };
                        popup::paint(mem, wd, ht, dpi, &view, dark);
                        true
                    }
                    Err(_) => false,
                });
                if painted {
                    BitBlt(hdc, 0, 0, wd, ht, mem, 0, 0, SRCCOPY);
                }
                SelectObject(mem, old);
                DeleteObject(bmp);
                DeleteDC(mem);
            }
            EndPaint(hwnd, &ps);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

// ---------------- Меню ----------------

unsafe fn show_menu(hwnd: HWND) {
    let tr = t();
    let mode = APP.with(|a| a.borrow().icon_mode);
    let interval = INTERVAL_MS.load(Ordering::Relaxed);
    let lang_set = LANG_SETTING.load(Ordering::Relaxed);
    let menu = CreatePopupMenu();
    let sub_mode = CreatePopupMenu();
    let sub_int = CreatePopupMenu();
    let sub_lang = CreatePopupMenu();
    let sub_rep = CreatePopupMenu();
    let chk = |on: bool| if on { MF_CHECKED } else { MF_UNCHECKED };
    let add = |m: HMENU, flags: u32, id: usize, text: &str| {
        AppendMenuW(m, flags, id, w(text).as_ptr());
    };

    add(sub_mode, MF_STRING | chk(mode == 0), CMD_MODE_W, tr.m_icon_w);
    add(sub_mode, MF_STRING | chk(mode == 1), CMD_MODE_P, tr.m_icon_p);
    add(sub_int, MF_STRING | chk(interval == 1000), CMD_INT_1, tr.m_1s);
    add(sub_int, MF_STRING | chk(interval == 2000), CMD_INT_2, tr.m_2s);
    add(sub_int, MF_STRING | chk(interval == 5000), CMD_INT_5, tr.m_5s);
    add(sub_lang, MF_STRING | chk(lang_set == 0), CMD_LANG_AUTO, tr.m_lang_auto);
    AppendMenuW(sub_lang, MF_SEPARATOR, 0, null());
    for (i, (name, _)) in i18n::LANG_NAMES.iter().enumerate() {
        add(sub_lang, MF_STRING | chk(lang_set == i as u32 + 1), CMD_LANG_AUTO + 1 + i, name);
    }

    let months = usage::months_available(&data_dir(), &today()[..7]);
    for (i, m) in months.iter().take(24).enumerate() {
        add(sub_rep, MF_STRING, CMD_REPORT_BASE + i, &report::month_title(m));
    }
    AppendMenuW(sub_rep, MF_SEPARATOR, 0, null());
    add(sub_rep, MF_STRING, CMD_OPEN_FOLDER, tr.m_open_folder);

    add(menu, MF_STRING | MF_GRAYED, 0, &format!("BatteryStats {} · by {}", VERSION, AUTHOR));
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    add(menu, MF_STRING, CMD_SHOW, tr.m_details);
    SetMenuDefaultItem(menu, CMD_SHOW as u32, 0);
    add(menu, MF_POPUP, sub_rep as usize, tr.m_reports);
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    add(menu, MF_POPUP, sub_mode as usize, tr.m_icon);
    add(menu, MF_POPUP, sub_int as usize, tr.m_interval);
    add(menu, MF_POPUP, sub_lang as usize, tr.m_lang);
    add(menu, MF_STRING | chk(autostart_enabled()), CMD_AUTOSTART, tr.m_autostart);
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    add(menu, MF_STRING, CMD_EXIT, tr.m_exit);

    let mut pt: POINT = zeroed();
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, null()) as usize;
    PostMessageW(hwnd, WM_NULL, 0, 0);
    DestroyMenu(menu); // уничтожает и подменю

    match cmd {
        CMD_SHOW => show_popup(),
        CMD_INT_1 | CMD_INT_2 | CMD_INT_5 => {
            let ms = match cmd {
                CMD_INT_1 => 1000,
                CMD_INT_2 => 2000,
                _ => 5000,
            };
            INTERVAL_MS.store(ms, Ordering::Relaxed);
            reg_set_dword(APP_KEY, "IntervalMs", ms);
            wake_poller();
        }
        CMD_MODE_W | CMD_MODE_P => {
            let m = if cmd == CMD_MODE_P { 1 } else { 0 };
            APP.with(|a| a.borrow_mut().icon_mode = m);
            reg_set_dword(APP_KEY, "IconMode", m);
            tray_update(hwnd);
        }
        c if (CMD_LANG_AUTO..=CMD_LANG_AUTO + 3).contains(&c) => {
            let v = (c - CMD_LANG_AUTO) as u32;
            reg_set_dword(APP_KEY, "Language", v);
            apply_lang(v);
            tray_update(hwnd);
            InvalidateRect(popup_hwnd(), null(), 0);
        }
        CMD_AUTOSTART => set_autostart(!autostart_enabled()),
        CMD_OPEN_FOLDER => {
            let dir = data_dir();
            let _ = std::fs::create_dir_all(&dir);
            shell_open(&dir.to_string_lossy());
        }
        c if c >= CMD_REPORT_BASE && c < CMD_REPORT_BASE + months.len().min(24) => {
            open_report(&months[c - CMD_REPORT_BASE]);
        }
        CMD_EXIT => {
            DestroyWindow(hwnd);
        }
        _ => {}
    }
}

// ---------------- Главное окно (скрытое) ----------------

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let tc = TASKBAR_CREATED.load(Ordering::Relaxed);
    if tc != 0 && msg == tc {
        tray_add(hwnd);
        return 0;
    }
    match msg {
        WM_TRAY => {
            match (lp & 0xFFFF) as u32 {
                WM_LBUTTONUP => toggle_popup(),
                WM_RBUTTONUP => {
                    hide_popup();
                    show_menu(hwnd);
                }
                _ => {}
            }
            0
        }
        WM_DATA => {
            let snap = LATEST.lock().ok().and_then(|mut g| g.take());
            if let Some(snap) = snap {
                APP.with(|a| {
                    if let Ok(mut a) = a.try_borrow_mut() {
                        let v = if snap.present { snap.power_w.unwrap_or(0.0) } else { 0.0 };
                        if a.history.len() >= popup::HISTORY {
                            a.history.pop_front();
                        }
                        a.history.push_back(v);
                        a.snap = snap;
                    }
                });
                tray_update(hwnd);
                if DEMO.swap(false, Ordering::Relaxed) {
                    show_popup();
                }
                resize_popup_if_needed();
            }
            0
        }
        WM_PROCS => {
            let st = PROCS.lock().ok().and_then(|mut g| g.take());
            if let Some(st) = st {
                APP.with(|a| {
                    if let Ok(mut a) = a.try_borrow_mut() {
                        a.procs = Some(st);
                    }
                });
                let popup = popup_hwnd();
                if IsWindowVisible(popup) != 0 {
                    InvalidateRect(popup, null(), 0);
                }
            }
            0
        }
        WM_POWERBROADCAST => {
            if wp == PBT_APMSUSPEND as usize {
                save_usage();
            }
            wake_poller();
            1
        }
        WM_QUERYENDSESSION => 1,
        WM_ENDSESSION => {
            if wp != 0 {
                save_usage();
            }
            0
        }
        WM_SETTINGCHANGE | WM_DISPLAYCHANGE => {
            wake_poller();
            0
        }
        WM_DESTROY => {
            save_usage();
            tray_remove(hwnd);
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn main() {
    let demo = std::env::args().any(|a| a == "--demo");
    DEMO.store(demo, Ordering::Relaxed);
    DEMO_DATA.store(demo, Ordering::Relaxed);
    apply_lang(reg_get_dword(APP_KEY, "Language").unwrap_or(0).min(3));
    #[cfg(feature = "devtools")]
    {
        let args: Vec<String> = std::env::args().collect();
        if let Some(i) = args.iter().position(|a| a == "--report-test") {
            // Синтетический месяц → отчёты на трёх языках.
            let dir = std::path::PathBuf::from(&args[i + 1]);
            let mut m = usage::Month::new();
            let apps = [
                ("chrome", "Google Chrome", 0.31),
                ("Code", "Visual Studio Code", 0.14),
                ("Telegram", "Telegram Desktop", 0.06),
                ("svchost", "Хост-процесс для служб Windows", 0.05),
                ("MsMpEng", "Antimalware Service Executable", 0.04),
                ("dwm", "Desktop Window Manager", 0.04),
                ("System", "", 0.03),
                ("Spotify", "Spotify", 0.03),
                ("explorer", "Проводник", 0.02),
                ("Zoom", "Zoom Meetings", 0.02),
            ];
            for d in 1..=6u32 {
                if d == 3 { continue; }
                let date = format!("2026-11-{:02}", d);
                let wh = 18.0 + (d as f64 * 7.3) % 25.0;
                let mut day = usage::Day { battery_s: wh / 11.5 * 3600.0, battery_wh: wh, ..Default::default() };
                let mut used = 0.0;
                for (k, n, sh) in apps {
                    let e = wh * sh * (0.8 + (d as f64 * 0.13) % 0.4);
                    used += e;
                    day.apps.insert(k.to_string(), usage::AppUse { name: n.to_string(), wh: e, cpu_s: e * 400.0 });
                }
                for j in 0..14 {
                    day.apps.insert(format!("tool{}", j), usage::AppUse { name: format!("Утилита {}", j), wh: 0.05, cpu_s: 20.0 });
                    used += 0.05;
                }
                day.apps.insert(usage::OTHER.to_string(), usage::AppUse { name: String::new(), wh: (wh - used).max(0.0), cpu_s: 0.0 });
                m.insert(date, day);
            }
            let months = vec!["2026-11".to_string(), "2026-10".to_string()];
            for (code, l) in [("ru", Lang::Ru), ("en", Lang::En), ("pl", Lang::Pl)] {
                i18n::set_lang(l);
                let _ = std::fs::write(dir.join(format!("report-{}.html", code)), report::render("2026-11", &m, Some(46.2), &months));
            }
            i18n::set_lang(Lang::Ru);
            let _ = std::fs::write(dir.join("report-empty.html"), report::render("2026-10", &usage::Month::new(), Some(46.2), &months));
            return;
        }
        if let Some(i) = args.iter().position(|a| a == "--icon-dump") {
            let dir = std::path::PathBuf::from(&args[i + 1]);
            for (n, t) in ["12", "8,5", "38", "100", "0", "\u{2014}"].iter().enumerate() {
                for sz in [16, 20, 24, 32, 48, 64] {
                    let px = unsafe { icon::render(t, 0xFFFFFF, sz) };
                    let _ = std::fs::write(dir.join(format!("icon_{}_{}.bgra", n, sz)), px);
                }
            }
            return;
        }
    }
    unsafe {
        // Один экземпляр.
        let mname = w("Local\\BatteryStats.SingleInstance");
        let _mutex = CreateMutexW(null(), 0, mname.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }

        if let Some(ms) = reg_get_dword(APP_KEY, "IntervalMs") {
            if [1000, 2000, 5000].contains(&ms) {
                INTERVAL_MS.store(ms, Ordering::Relaxed);
            }
        }
        let mode = reg_get_dword(APP_KEY, "IconMode").unwrap_or(0).min(1);
        if let Ok(mut g) = USAGE.lock() {
            *g = Some(Usage::load(&data_dir(), &today()));
        }
        APP.with(|a| a.borrow_mut().icon_mode = mode);

        let mut token: usize = 0;
        let input = GdiplusStartupInput { GdiplusVersion: 1, DebugEventCallback: 0, SuppressBackgroundThread: 0, SuppressExternalCodecs: 0 };
        GdiplusStartup(&mut token, &input, null_mut());

        let hinst = GetModuleHandleW(null());
        let app_icon = LoadIconW(hinst, 1 as *const u16);

        let main_cls = w("BatteryStatsMain");
        let mut wc: WNDCLASSEXW = zeroed();
        wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(main_proc);
        wc.hInstance = hinst;
        wc.hIcon = app_icon;
        wc.lpszClassName = main_cls.as_ptr();
        RegisterClassExW(&wc);

        let pop_cls = w("BatteryStatsPopup");
        let mut pc: WNDCLASSEXW = zeroed();
        pc.cbSize = size_of::<WNDCLASSEXW>() as u32;
        pc.style = CS_DROPSHADOW;
        pc.lpfnWndProc = Some(popup_proc);
        pc.hInstance = hinst;
        pc.hIcon = app_icon;
        pc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
        pc.lpszClassName = pop_cls.as_ptr();
        RegisterClassExW(&pc);

        TASKBAR_CREATED.store(RegisterWindowMessageW(w("TaskbarCreated").as_ptr()), Ordering::Relaxed);

        let title = w("BatteryStats");
        let main = CreateWindowExW(0, main_cls.as_ptr(), title.as_ptr(), WS_OVERLAPPED, 0, 0, 0, 0,
            null_mut(), null_mut(), hinst, null());
        MAIN_HWND.store(main as isize, Ordering::Relaxed);

        let popup = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST, pop_cls.as_ptr(), title.as_ptr(), WS_POPUP,
            0, 0, 320, 500, null_mut(), null_mut(), hinst, null());
        POPUP_HWND.store(popup as isize, Ordering::Relaxed);
        // Windows 11: скруглённые углы и тёмная рамка в тёмной теме.
        let round: u32 = 2; // DWMWCP_ROUND
        DwmSetWindowAttribute(popup, 33, &round as *const u32 as *const _, 4);
        let dark: i32 = if apps_are_dark() { 1 } else { 0 };
        DwmSetWindowAttribute(popup, 20, &dark as *const i32 as *const _, 4);

        tray_add(main);

        // Поток опроса батареи.
        let main_raw = main as isize;
        std::thread::spawn(move || {
            let mut reader = Reader::new(demo);
            let mut last = Instant::now();
            loop {
                let snap = reader.read();
                // Интегрируем реальную мощность разряда → Вт·ч.
                let now = Instant::now();
                let cap = INTERVAL_MS.load(Ordering::Relaxed) as f64 / 1000.0 * 2.0 + 1.0;
                let dt = now.duration_since(last).as_secs_f64().min(cap);
                last = now;
                if snap.discharging {
                    if let Some(p) = snap.power_w.filter(|p| *p < 0.0) {
                        if let Ok(mut g) = PENDING.lock() {
                            g.0 += (-p) as f64 * dt / 3600.0;
                            g.1 += dt;
                        }
                    }
                }
                if DISCHARGING.swap(snap.discharging, Ordering::Relaxed) != snap.discharging {
                    wake_procs();
                }
                if let Ok(mut g) = LATEST.lock() {
                    *g = Some(snap);
                }
                PostMessageW(main_raw as HWND, WM_DATA, 0, 0);
                let ms = INTERVAL_MS.load(Ordering::Relaxed) as u64;
                if let Ok(g) = WAKE_M.lock() {
                    if let Ok((mut g, _)) = WAKE_CV.wait_timeout_while(g, Duration::from_millis(ms), |woken| !*woken) {
                        *g = false;
                    }
                }
            }
        });

        // Поток замера процессов: пока открыто окно (каждые 1,5 с) и пока ноутбук
        // работает от батареи (каждые 10 с — для месячной статистики).
        std::thread::spawn(move || {
            let mut sampler = Sampler::new(demo);
            loop {
                let open = POPUP_OPEN.load(Ordering::Relaxed);
                let dis = DISCHARGING.load(Ordering::Relaxed);
                let pending = PENDING.lock().map(|g| g.0 > 0.0 || g.1 > 0.0).unwrap_or(false);
                let mut not_ready = false;
                if open || dis || pending {
                    let sample = sampler.sample_raw();
                    not_ready = matches!(sample, Sample::NotReady);
                    if !not_ready {
                        let (wh, secs) = PENDING.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or((0.0, 0.0));
                        attribute(&mut sampler, &sample, wh, secs);
                    }
                    if open {
                        let st = match &sample {
                            Sample::Data(g) => Some(sampler.ui_stats(g)),
                            Sample::Unavailable => Some(ProcStats { unavailable: true, ..Default::default() }),
                            Sample::NotReady => None,
                        };
                        if let Some(st) = st {
                            if let Ok(mut g) = PROCS.lock() {
                                *g = Some(st);
                            }
                            PostMessageW(main_raw as HWND, WM_PROCS, 0, 0);
                        }
                    }
                }
                if let Ok(mut g) = USAGE.lock() {
                    if let Some(u) = g.as_mut() {
                        if u.needs_save() {
                            u.save(&data_dir(), &today());
                        }
                    }
                }
                let next_ms = if not_ready {
                    600
                } else if open {
                    1500
                } else if dis {
                    10_000
                } else {
                    60_000
                };
                if let Ok(g) = PROC_M.lock() {
                    if let Ok((mut g, _)) = PROC_CV.wait_timeout_while(g, Duration::from_millis(next_ms), |w| !*w) {
                        *g = false;
                    }
                }
            }
        });

        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        GdiplusShutdown(token);
    }
}
