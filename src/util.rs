use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::*;

/// UTF-16 строка с завершающим нулём.
pub fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Число с одной/двумя цифрами после запятой (разделитель — по языку).
pub fn f1(v: f32) -> String {
    dec(format!("{:.1}", v))
}
pub fn f2(v: f32) -> String {
    dec(format!("{:.2}", v))
}
pub fn dec(s: String) -> String {
    if crate::i18n::t().decimal_comma {
        s.replace('.', ",")
    } else {
        s
    }
}

pub fn fmt_duration(min: u32) -> String {
    let t = crate::i18n::t();
    let h = min / 60;
    let m = min % 60;
    if h == 0 {
        crate::i18n::fill(t.dur_min, &[&m.to_string()])
    } else {
        crate::i18n::fill(t.dur_hmin, &[&h.to_string(), &m.to_string()])
    }
}

/// Локальные дата и время: (год, месяц, день, час, минута).
pub fn local_now() -> (u16, u16, u16, u16, u16) {
    let mut st: windows_sys::Win32::Foundation::SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut st) };
    (st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute)
}

pub fn today() -> String {
    let (y, m, d, _, _) = local_now();
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// %LOCALAPPDATA%\BatteryStats (в демо-режиме — BatteryStats-demo).
pub fn data_dir() -> std::path::PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir());
    let name = if DEMO_DATA.load(std::sync::atomic::Ordering::Relaxed) { "BatteryStats-demo" } else { "BatteryStats" };
    base.join(name)
}

pub static DEMO_DATA: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// COLORREF из RGB.
pub const fn rgb(c: u32) -> u32 {
    let r = (c >> 16) & 0xFF;
    let g = (c >> 8) & 0xFF;
    let b = c & 0xFF;
    r | (g << 8) | (b << 16)
}

/// ARGB для GDI+.
pub const fn argb(a: u32, c: u32) -> u32 {
    (a << 24) | (c & 0x00FF_FFFF)
}

pub const APP_KEY: &str = "Software\\BatteryStats";
pub const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
pub const RUN_VALUE: &str = "BatteryStats";

pub fn reg_get_dword(subkey: &str, name: &str) -> Option<u32> {
    let sk = w(subkey);
    let nm = w(name);
    let mut v: u32 = 0;
    let mut size: u32 = 4;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            sk.as_ptr(),
            nm.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut v as *mut u32 as *mut _,
            &mut size,
        )
    };
    if r == ERROR_SUCCESS {
        Some(v)
    } else {
        None
    }
}

pub fn reg_set_dword(subkey: &str, name: &str, v: u32) {
    let sk = w(subkey);
    let nm = w(name);
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            sk.as_ptr(),
            nm.as_ptr(),
            REG_DWORD,
            &v as *const u32 as *const _,
            4,
        );
    }
}

pub fn reg_get_string(subkey: &str, name: &str) -> Option<String> {
    let sk = w(subkey);
    let nm = w(name);
    let mut buf = vec![0u16; 1024];
    let mut size: u32 = (buf.len() * 2) as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            sk.as_ptr(),
            nm.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut _,
            &mut size,
        )
    };
    if r != ERROR_SUCCESS {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

pub fn reg_set_string(subkey: &str, name: &str, value: &str) {
    let sk = w(subkey);
    let nm = w(name);
    let v = w(value);
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            sk.as_ptr(),
            nm.as_ptr(),
            REG_SZ,
            v.as_ptr() as *const _,
            (v.len() * 2) as u32,
        );
    }
}

pub fn reg_delete_value(subkey: &str, name: &str) {
    let sk = w(subkey);
    let nm = w(name);
    unsafe {
        RegDeleteKeyValueW(HKEY_CURRENT_USER, sk.as_ptr(), nm.as_ptr());
    }
}

/// Тёмная ли панель задач (SystemUsesLightTheme = 0).
pub fn taskbar_is_dark() -> bool {
    reg_get_dword(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
        "SystemUsesLightTheme",
    )
    .map(|v| v == 0)
    .unwrap_or(true)
}

/// Тёмная ли тема приложений (AppsUseLightTheme = 0).
pub fn apps_are_dark() -> bool {
    reg_get_dword(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
        "AppsUseLightTheme",
    )
    .map(|v| v == 0)
    .unwrap_or(false)
}

pub fn exe_path() -> String {
    let mut buf = vec![0u16; 1024];
    let n = unsafe {
        windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW(
            std::ptr::null_mut(),
            buf.as_mut_ptr(),
            buf.len() as u32,
        )
    } as usize;
    String::from_utf16_lossy(&buf[..n])
}

pub fn autostart_enabled() -> bool {
    match reg_get_string(RUN_KEY, RUN_VALUE) {
        Some(v) => v.trim_matches('"').eq_ignore_ascii_case(&exe_path()),
        None => false,
    }
}

pub fn set_autostart(on: bool) {
    if on {
        reg_set_string(RUN_KEY, RUN_VALUE, &format!("\"{}\"", exe_path()));
    } else {
        reg_delete_value(RUN_KEY, RUN_VALUE);
    }
}
