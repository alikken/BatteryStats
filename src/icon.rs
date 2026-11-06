//! Рендер числа в иконку трея (сглаженный текст, прозрачный фон).
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::util::w;

unsafe fn dib(hdc: HDC, wdt: i32, hgt: i32, bits: &mut *mut u8) -> HBITMAP {
    let mut bmi: BITMAPINFO = zeroed();
    bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = wdt;
    bmi.bmiHeader.biHeight = -hgt; // сверху вниз
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;
    let mut p: *mut core::ffi::c_void = null_mut();
    let h = CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut p, null_mut(), 0);
    *bits = p as *mut u8;
    h
}

/// `color` — 0xRRGGBB. `size` — сторона иконки в пикселях. Возвращает BGRA (прямая альфа).
pub unsafe fn render(text: &str, color: u32, size: i32) -> Vec<u8> {
    let sw = size * 4;
    let sh = size * 2;
    let screen = GetDC(null_mut());
    let mem = CreateCompatibleDC(screen);
    let mut sbits: *mut u8 = null_mut();
    let sbmp = dib(mem, sw, sh, &mut sbits);
    let old = SelectObject(mem, sbmp);
    SetBkMode(mem, TRANSPARENT as i32);
    SetTextColor(mem, 0x00FF_FFFF);
    let face = w("Segoe UI");
    let txt: Vec<u16> = text.encode_utf16().collect();

    // Подбираем максимальный кегль, при котором «чернила» влезают в квадрат.
    let mut chosen: Option<(i32, i32, i32, i32)> = None; // x0,y0,x1,y1
    let mut px = (size as f32 * 1.25) as i32;
    while px >= 6 {
        std::ptr::write_bytes(sbits, 0, (sw * sh * 4) as usize);
        let font = CreateFontW(
            -px, 0, 0, 0, FW_BOLD as i32, 0, 0, 0,
            DEFAULT_CHARSET as u32, OUT_TT_PRECIS as u32, CLIP_DEFAULT_PRECIS as u32,
            ANTIALIASED_QUALITY as u32, DEFAULT_PITCH as u32, face.as_ptr(),
        );
        let of = SelectObject(mem, font);
        TextOutW(mem, 2, 2, txt.as_ptr(), txt.len() as i32);
        GdiFlush();
        SelectObject(mem, of);
        DeleteObject(font);

        let (mut x0, mut y0, mut x1, mut y1) = (sw, sh, -1, -1);
        for y in 0..sh {
            for x in 0..sw {
                let i = ((y * sw + x) * 4) as usize;
                let a = (*sbits.add(i)).max(*sbits.add(i + 1)).max(*sbits.add(i + 2));
                if a > 8 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        if x1 < 0 {
            break; // пустой текст
        }
        // Высоту цифр ограничиваем ~80% — одна цифра не должна быть крупнее двух.
        let max_h = ((size as f32) * 0.8).round() as i32;
        if x1 - x0 + 1 <= size && y1 - y0 + 1 <= max_h {
            chosen = Some((x0, y0, x1, y1));
            break;
        }
        px -= 1;
    }

    // Итоговая иконка: цвет + альфа из покрытия глифа.
    let mut out = vec![0u8; (size * size * 4) as usize];
    let ibits = out.as_mut_ptr();
    if let Some((x0, y0, x1, y1)) = chosen {
        let gw = x1 - x0 + 1;
        let gh = y1 - y0 + 1;
        let ox = (size - gw) / 2;
        let oy = (size - gh + 1) / 2;
        let (r, g, b) = (((color >> 16) & 0xFF) as u8, ((color >> 8) & 0xFF) as u8, (color & 0xFF) as u8);
        for y in 0..gh {
            for x in 0..gw {
                let si = (((y0 + y) * sw + (x0 + x)) * 4) as usize;
                let a = (*sbits.add(si)).max(*sbits.add(si + 1)).max(*sbits.add(si + 2));
                let di = (((oy + y) * size + (ox + x)) * 4) as usize;
                *ibits.add(di) = b;
                *ibits.add(di + 1) = g;
                *ibits.add(di + 2) = r;
                *ibits.add(di + 3) = a;
            }
        }
    }
    SelectObject(mem, old);
    DeleteObject(sbmp);
    DeleteDC(mem);
    ReleaseDC(null_mut(), screen);
    out
}

pub unsafe fn text_icon(text: &str, color: u32, size: i32) -> HICON {
    let px = render(text, color, size);
    let screen = GetDC(null_mut());
    let mut bits: *mut u8 = null_mut();
    let bmp = dib(screen, size, size, &mut bits);
    std::ptr::copy_nonoverlapping(px.as_ptr(), bits, px.len());
    let mask = CreateBitmap(size, size, 1, 1, null());
    let ii = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: bmp };
    let icon = CreateIconIndirect(&ii);
    DeleteObject(bmp);
    DeleteObject(mask);
    ReleaseDC(null_mut(), screen);
    icon
}
