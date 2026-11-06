//! Всплывающее окно: мощность, график, детали батареи и энергоёмкие процессы.
use std::collections::VecDeque;
use std::mem::zeroed;
use std::ptr::null_mut;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::Graphics::GdiPlus::*;
use windows_sys::Win32::UI::WindowsAndMessaging::{DrawIconEx, DI_NORMAL};

use crate::battery::Snapshot;
use crate::procs::{ProcStats, SYSTEM_SHARE};
use crate::i18n::{fill, t};
use crate::util::{argb, f1, f2, fmt_duration, rgb, w};
use std::sync::atomic::{AtomicI32, Ordering};

/// Прямоугольник ссылки «Отчёт за месяц» (клиентские пиксели), для клика.
pub static LINK: [AtomicI32; 4] = [AtomicI32::new(0), AtomicI32::new(0), AtomicI32::new(0), AtomicI32::new(0)];

pub fn link_hit(x: i32, y: i32) -> bool {
    let r: Vec<i32> = LINK.iter().map(|a| a.load(Ordering::Relaxed)).collect();
    r[2] > r[0] && x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
}

pub const HISTORY: usize = 300;
pub const WIDTH: i32 = 340;
const PAD: i32 = 20;
const TILES_TOP: i32 = 280;
const TILE_H: i32 = 46;
pub const PROC_ROW_H: i32 = 30;

pub struct Theme {
    pub bg: u32,
    pub text: u32,
    pub sub: u32,
    pub sep: u32,
    pub panel: u32,
    pub track: u32,
    pub accent: u32,
    pub green: u32,
    pub orange: u32,
    pub red: u32,
}

pub fn theme(dark: bool) -> Theme {
    if dark {
        Theme {
            bg: 0x1F1F22, text: 0xF2F2F5, sub: 0x9A9AA2, sep: 0x2E2E33, panel: 0x26262B,
            track: 0x34343A, accent: 0x5AA2FF, green: 0x34C77B, orange: 0xFFA43A, red: 0xFF5A52,
        }
    } else {
        Theme {
            bg: 0xFFFFFF, text: 0x1D1D22, sub: 0x76767E, sep: 0xEEEEF2, panel: 0xF6F7FA,
            track: 0xE9EBF0, accent: 0x2F7CF6, green: 0x22A55B, orange: 0xE8890C, red: 0xE5483F,
        }
    }
}

pub struct View<'a> {
    pub snap: &'a Snapshot,
    pub history: &'a VecDeque<f32>,
    pub interval_ms: u32,
    pub procs: Option<&'a ProcStats>,
    pub proc_rows: i32,
}

fn chemistry_name(c: &str) -> String {
    match c.to_ascii_uppercase().as_str() {
        "LION" | "LI-I" | "LI" => "Li-ion".into(),
        "LIP" | "LIPO" | "LIPOL" => "Li-polymer".into(),
        "NIMH" => "NiMH".into(),
        "PBAC" => t().lead_acid.into(),
        "" => String::new(),
        other => other.to_string(),
    }
}

/// Плитки с деталями (до 6 штук, сетка 2×3).
pub fn tiles(s: &Snapshot) -> Vec<(&'static str, String)> {
    let tr = t();
    let dash = || "—".to_string();
    let mut r: Vec<(&'static str, String)> = vec![
        (tr.tile_voltage, s.voltage_v.map(|v| format!("{} {}", f2(v), tr.u_v)).unwrap_or_else(dash)),
        (tr.tile_current, s.current_a.map(|a| format!("{} {}", f2(a), tr.u_a)).unwrap_or_else(dash)),
        (
            tr.tile_energy,
            match (s.energy_wh, s.full_wh) {
                (Some(e), Some(f)) => format!("{} / {} {}", f1(e), f1(f), tr.u_wh),
                (Some(e), None) => format!("{} {}", f1(e), tr.u_wh),
                _ => dash(),
            },
        ),
        (tr.tile_health, s.health().map(|h| format!("{:.0} %", h)).unwrap_or_else(dash)),
    ];
    if let Some(c) = s.cycles {
        r.push((tr.tile_cycles, c.to_string()));
    }
    if let Some(tc) = s.temp_c {
        r.push((tr.tile_temp, format!("{} °C", f1(tc))));
    }
    if let Some(d) = s.design_wh {
        r.push((tr.tile_design, format!("{} {}", f1(d), tr.u_wh)));
    }
    let chem = chemistry_name(&s.chemistry);
    if !chem.is_empty() {
        r.push((tr.tile_type, chem));
    }
    r.truncate(6);
    r
}

pub struct Layout {
    pub sep: i32,
    pub proc_hdr: i32,
    pub proc_top: i32,
    pub foot: i32,
    pub height: i32,
}

pub fn layout(s: &Snapshot, proc_rows: i32) -> Layout {
    let tile_rows = (tiles(s).len() as i32 + 1) / 2;
    let sep = TILES_TOP + tile_rows * TILE_H + 6;
    let proc_hdr = sep + 26;
    let proc_top = proc_hdr + 8;
    let foot = proc_top + proc_rows * PROC_ROW_H + 24;
    Layout { sep, proc_hdr, proc_top, foot, height: foot + 16 }
}

pub fn status_text(s: &Snapshot) -> &'static str {
    let tr = t();
    if !s.present {
        tr.st_none
    } else if s.charging {
        tr.st_charging
    } else if s.discharging {
        tr.st_discharging
    } else if s.on_ac {
        tr.st_ac
    } else {
        tr.st_idle
    }
}

pub fn subtitle(s: &Snapshot) -> String {
    let tr = t();
    if !s.present {
        return tr.sub_nodata.into();
    }
    match (s.charging, s.discharging, s.time_left_min) {
        (true, _, Some(m)) => fill(tr.sub_to_full, &[&fmt_duration(m)]),
        (_, true, Some(m)) => fill(tr.sub_to_empty, &[&fmt_duration(m)]),
        (true, _, None) => tr.sub_charging.into(),
        (_, true, None) => tr.sub_on_battery.into(),
        _ if s.percent.unwrap_or(0.0) >= 95.0 => tr.sub_full_ac.into(),
        _ => tr.sub_ac_not_charging.into(),
    }
}

fn nice_ceil(v: f32) -> f32 {
    for s in [5.0, 10.0, 15.0, 20.0, 30.0, 40.0, 50.0, 60.0, 80.0, 100.0, 150.0, 200.0, 300.0] {
        if v <= s {
            return s;
        }
    }
    (v / 100.0).ceil() * 100.0
}

/// Мощность разряда (Вт, > 0), если ноутбук работает от батареи.
pub fn discharge_w(s: &Snapshot) -> Option<f32> {
    if s.discharging {
        s.power_w.map(|p| -p).filter(|p| *p > 0.05)
    } else {
        None
    }
}

static VARIABLE_FONT: OnceLock<bool> = OnceLock::new();

/// Есть ли Segoe UI Variable (Windows 11).
unsafe fn has_variable_font(hdc: HDC) -> bool {
    *VARIABLE_FONT.get_or_init(|| {
        let name = "Segoe UI Variable Text";
        let face = w(name);
        let f = CreateFontW(-12, 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET as u32, OUT_TT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32, CLEARTYPE_QUALITY as u32, DEFAULT_PITCH as u32, face.as_ptr());
        let of = SelectObject(hdc, f);
        let mut buf = [0u16; 64];
        let n = GetTextFaceW(hdc, buf.len() as i32, buf.as_mut_ptr());
        SelectObject(hdc, of);
        DeleteObject(f);
        n > 0 && String::from_utf16_lossy(&buf[..(n as usize).saturating_sub(1).min(63)]).eq_ignore_ascii_case(name)
    })
}

struct Fonts {
    title: HFONT,
    big: HFONT,
    unit: HFONT,
    pct: HFONT,
    body: HFONT,
    body_b: HFONT,
    value: HFONT,
    small: HFONT,
}

impl Fonts {
    unsafe fn new(hdc: HDC, k: f32) -> Fonts {
        let var = has_variable_font(hdc);
        let text_face = w(if var { "Segoe UI Variable Text" } else { "Segoe UI" });
        let disp_face = w(if var { "Segoe UI Variable Display" } else { "Segoe UI" });
        let mk = |px: f32, weight: i32, face: &Vec<u16>| {
            CreateFontW(-(px * k).round() as i32, 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET as u32,
                OUT_TT_PRECIS as u32, CLIP_DEFAULT_PRECIS as u32, CLEARTYPE_QUALITY as u32,
                DEFAULT_PITCH as u32, face.as_ptr())
        };
        Fonts {
            title: mk(14.0, 600, &text_face),
            big: mk(40.0, 600, &disp_face),
            unit: mk(18.0, 500, &disp_face),
            pct: mk(22.0, 600, &disp_face),
            body: mk(13.0, 400, &text_face),
            body_b: mk(13.0, 600, &text_face),
            value: mk(14.5, 600, &text_face),
            small: mk(11.5, 400, &text_face),
        }
    }
    unsafe fn free(&self) {
        for f in [self.title, self.big, self.unit, self.pct, self.body, self.body_b, self.value, self.small] {
            DeleteObject(f);
        }
    }
}

unsafe fn text_width(hdc: HDC, font: HFONT, s: &str) -> i32 {
    let t: Vec<u16> = s.encode_utf16().collect();
    let of = SelectObject(hdc, font);
    let mut sz: SIZE = zeroed();
    GetTextExtentPoint32W(hdc, t.as_ptr(), t.len() as i32, &mut sz);
    SelectObject(hdc, of);
    sz.cx
}

/// Текст по базовой линии. `right` — выравнивание по правому краю.
unsafe fn text_at(hdc: HDC, font: HFONT, color: u32, s: &str, x: i32, baseline: i32, right: bool) {
    let t: Vec<u16> = s.encode_utf16().collect();
    let of = SelectObject(hdc, font);
    SetTextColor(hdc, rgb(color));
    SetTextAlign(hdc, TA_BASELINE | if right { TA_RIGHT } else { TA_LEFT });
    TextOutW(hdc, x, baseline, t.as_ptr(), t.len() as i32);
    SelectObject(hdc, of);
}

unsafe fn text_rect(hdc: HDC, font: HFONT, color: u32, s: &str, mut r: RECT, flags: u32) {
    let t: Vec<u16> = s.encode_utf16().collect();
    let of = SelectObject(hdc, font);
    SetTextColor(hdc, rgb(color));
    SetTextAlign(hdc, TA_LEFT | TA_TOP);
    DrawTextW(hdc, t.as_ptr(), t.len() as i32, &mut r, flags | DT_SINGLELINE | DT_NOPREFIX | DT_VCENTER);
    SelectObject(hdc, of);
}

// ---------- GDI+ ----------

unsafe fn round_path(x: f32, y: f32, wd: f32, h: f32, r: f32) -> *mut GpPath {
    let mut p: *mut GpPath = null_mut();
    GdipCreatePath(FillModeAlternate, &mut p);
    let r = r.min(h / 2.0).min(wd / 2.0).max(0.1);
    let d = r * 2.0;
    GdipAddPathArc(p, x, y, d, d, 180.0, 90.0);
    GdipAddPathArc(p, x + wd - d, y, d, d, 270.0, 90.0);
    GdipAddPathArc(p, x + wd - d, y + h - d, d, d, 0.0, 90.0);
    GdipAddPathArc(p, x, y + h - d, d, d, 90.0, 90.0);
    GdipClosePathFigure(p);
    p
}

unsafe fn fill_round(g: *mut GpGraphics, x: f32, y: f32, wd: f32, h: f32, r: f32, color: u32) {
    if wd <= 0.5 || h <= 0.5 {
        return;
    }
    let p = round_path(x, y, wd, h, r);
    let mut b: *mut GpSolidFill = null_mut();
    GdipCreateSolidFill(color, &mut b);
    GdipFillPath(g, b as *mut GpBrush, p);
    GdipDeleteBrush(b as *mut GpBrush);
    GdipDeletePath(p);
}

unsafe fn hline(g: *mut GpGraphics, x1: f32, x2: f32, y: f32, color: u32, width: f32) {
    let mut pen: *mut GpPen = null_mut();
    GdipCreatePen1(color, width, UnitPixel, &mut pen);
    GdipDrawLine(g, pen, x1, y, x2, y);
    GdipDeletePen(pen);
}

/// Строка списка процессов, готовая к отрисовке.
struct PRow {
    name: String,
    icon: isize,
    detail: String,
    value: String,
    share: f32, // 0..1 для полоски
    system: bool,
}

fn proc_rows(v: &View) -> Vec<PRow> {
    let Some(ps) = v.procs else { return Vec::new() };
    let max_rows = v.proc_rows.max(1) as usize;
    let watts = discharge_w(v.snap);
    let denom = ps.total_impact + SYSTEM_SHARE;
    let n = if watts.is_some() { max_rows.saturating_sub(1) } else { max_rows };
    let top_impact = ps.rows.first().map(|r| r.impact()).unwrap_or(1.0).max(0.01);
    let mut out = Vec::new();
    let mut shown_w = 0.0;
    for r in ps.rows.iter().take(n) {
        let tr = t();
        let mut detail = format!("{} {:.0}%", tr.cpu, r.cpu);
        if r.gpu >= 1.0 {
            detail += &format!(" · {} {:.0}%", tr.gpu, r.gpu);
        }
        let value = match watts {
            Some(p) => {
                let wv = p * r.impact() / denom;
                shown_w += wv;
                format!("{} {}", f1(wv), t().u_w)
            }
            None => format!("{:.0} %", r.impact() / ps.total_impact.max(0.01) * 100.0),
        };
        out.push(PRow { name: r.name(), icon: r.icon, detail, value, share: r.impact() / top_impact, system: false });
    }
    if let Some(p) = watts {
        out.push(PRow {
            name: t().other_bucket.into(),
            icon: 0,
            detail: String::new(),
            value: format!("{} {}", f1((p - shown_w).max(0.0)), t().u_w),
            share: 0.0,
            system: true,
        });
    }
    out
}

pub unsafe fn paint(hdc: HDC, wpx: i32, hpx: i32, dpi: u32, v: &View, dark: bool) {
    let k = dpi as f32 / 96.0;
    let s = |x: i32| (x as f32 * k).round() as i32;
    let t = theme(dark);
    let snap = v.snap;
    let lay = layout(snap, v.proc_rows);

    // Фон
    let full = RECT { left: 0, top: 0, right: wpx, bottom: hpx };
    let bg = CreateSolidBrush(rgb(t.bg));
    FillRect(hdc, &full, bg);
    DeleteObject(bg);
    SetBkMode(hdc, TRANSPARENT as i32);

    let fonts = Fonts::new(hdc, k);
    let left = s(PAD);
    let right = wpx - s(PAD);

    let state_color = if snap.charging {
        t.green
    } else if snap.discharging {
        t.accent
    } else {
        t.sub
    };
    let pct = snap.percent.unwrap_or(0.0);
    let bar_color = if snap.charging {
        t.green
    } else if snap.discharging && pct < 10.0 {
        t.red
    } else if snap.discharging && pct < 20.0 {
        t.orange
    } else {
        t.accent
    };
    let prow = proc_rows(v);

    // ---- Графика (GDI+) ----
    let mut g: *mut GpGraphics = null_mut();
    GdipCreateFromHDC(hdc, &mut g);
    GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
    GdipSetPixelOffsetMode(g, PixelOffsetModeHalf);

    // Полоса заряда
    let bar_y = s(132) as f32;
    let bar_h = (6.0 * k).max(4.0);
    let bw = (right - left) as f32;
    fill_round(g, left as f32, bar_y, bw, bar_h, bar_h / 2.0, argb(0xFF, t.track));
    if snap.percent.is_some() {
        fill_round(g, left as f32, bar_y, (bw * pct / 100.0).max(bar_h), bar_h, bar_h / 2.0, argb(0xFF, bar_color));
    }

    // График мощности
    let gx = left as f32;
    let gy = s(178) as f32;
    let gw = bw;
    let gh = s(84) as f32;
    fill_round(g, gx, gy, gw, gh, 10.0 * k, argb(0xFF, t.panel));
    let panel = round_path(gx, gy, gw, gh, 10.0 * k);
    GdipSetClipPath(g, panel, CombineModeReplace);
    for frac in [0.25f32, 0.5, 0.75] {
        hline(g, gx, gx + gw, gy + gh * frac, argb(0xFF, t.sep), 1.0);
    }
    let hist: Vec<f32> = v.history.iter().copied().collect();
    let maxabs = hist.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    let ymax = nice_ceil((maxabs * 1.15).max(5.0));
    if hist.len() >= 2 {
        let n = hist.len();
        let step = gw / (HISTORY as f32 - 1.0);
        let base = gy + gh;
        let top_pad = 8.0 * k;
        let px = |i: usize| gx + gw - (n - 1 - i) as f32 * step;
        let py = |val: f32| base - (val.abs() / ymax) * (gh - top_pad);
        let col = |val: f32| {
            if val > 0.05 {
                t.green
            } else if val < -0.05 {
                t.accent
            } else {
                t.sub
            }
        };
        let mut a = 0usize;
        while a < n - 1 {
            let c = col(hist[a + 1]);
            let mut b = a + 1;
            while b + 1 < n && col(hist[b + 1]) == c {
                b += 1;
            }
            let mut pts: Vec<PointF> = Vec::with_capacity(b - a + 3);
            pts.push(PointF { X: px(a), Y: base });
            for i in a..=b {
                pts.push(PointF { X: px(i), Y: py(hist[i]) });
            }
            pts.push(PointF { X: px(b), Y: base });
            let p1 = PointF { X: 0.0, Y: gy };
            let p2 = PointF { X: 0.0, Y: base + 1.0 };
            let mut lb: *mut GpLineGradient = null_mut();
            GdipCreateLineBrush(&p1, &p2, argb(0x66, c), argb(0x08, c), WrapModeTileFlipXY, &mut lb);
            GdipFillPolygon(g, lb as *mut GpBrush, pts.as_ptr(), pts.len() as i32, FillModeAlternate);
            GdipDeleteBrush(lb as *mut GpBrush);
            let mut pen: *mut GpPen = null_mut();
            GdipCreatePen1(argb(0xFF, c), 1.8 * k, UnitPixel, &mut pen);
            GdipSetPenLineJoin(pen, LineJoinRound);
            GdipSetPenStartCap(pen, LineCapRound);
            GdipSetPenEndCap(pen, LineCapRound);
            GdipDrawLines(g, pen, pts[1..pts.len() - 1].as_ptr(), (pts.len() - 2) as i32);
            GdipDeletePen(pen);
            a = b;
        }
    }
    GdipResetClip(g);
    GdipDeletePath(panel);

    // Разделитель перед процессами
    hline(g, left as f32, right as f32, s(lay.sep) as f32, argb(0xFF, t.sep), 1.0);

    // Полоски доли нагрузки и заглушки иконок в списке процессов
    let icon_sz = s(16);
    let name_x = left + icon_sz + s(10);
    for (i, r) in prow.iter().enumerate() {
        let top = s(lay.proc_top + PROC_ROW_H * i as i32);
        let iy = top + (s(PROC_ROW_H) - icon_sz) / 2;
        if r.icon == 0 {
            fill_round(g, left as f32, iy as f32, icon_sz as f32, icon_sz as f32, 4.0 * k, argb(0xFF, t.track));
        }
        if !r.system && r.share > 0.0 {
            let by = (top + s(PROC_ROW_H) - s(3)) as f32;
            let full_w = (right - name_x) as f32;
            fill_round(g, name_x as f32, by, full_w, 2.0 * k, k, argb(0xFF, t.panel));
            fill_round(g, name_x as f32, by, (full_w * r.share.min(1.0)).max(2.0 * k), 2.0 * k, k, argb(0x70, t.accent));
        }
    }
    GdipDeleteGraphics(g);

    // ---- Текст (GDI, ClearType) ----
    let tr = crate::i18n::t();
    text_at(hdc, fonts.title, t.text, tr.battery, left, s(36), false);
    text_at(hdc, fonts.body, state_color, status_text(snap), right, s(36), true);

    let big = match snap.power_w {
        Some(p) => f1(p.abs()),
        None => "—".to_string(),
    };
    let big_base = s(92);
    text_at(hdc, fonts.big, t.text, &big, left, big_base, false);
    let bwid = text_width(hdc, fonts.big, &big);
    text_at(hdc, fonts.unit, t.sub, tr.u_w, left + bwid + s(6), big_base, false);
    let pct_s = match snap.percent {
        Some(p) => format!("{:.0}%", p),
        None => "—".into(),
    };
    text_at(hdc, fonts.pct, t.text, &pct_s, right, big_base, true);

    text_at(hdc, fonts.small, t.sub, &subtitle(snap), left, s(117), false);

    let span_min = (HISTORY as u32 * v.interval_ms) / 60_000;
    text_at(hdc, fonts.small, t.sub, &fill(tr.graph_label, &[&span_min.max(1).to_string()]), left, s(168), false);
    if !hist.is_empty() {
        let avg = hist.iter().map(|x| x.abs()).sum::<f32>() / hist.len() as f32;
        text_at(hdc, fonts.small, t.sub, &fill(tr.graph_stats, &[&f1(avg), &f1(maxabs), tr.u_w]), right, s(168), true);
    }

    // Плитки 2×N
    let col_w = (right - left) / 2;
    for (i, (label, value)) in tiles(snap).iter().enumerate() {
        let col = (i % 2) as i32;
        let row = (i / 2) as i32;
        let x = left + col * (col_w + s(6));
        let top = s(TILES_TOP + row * TILE_H);
        text_at(hdc, fonts.small, t.sub, label, x, top + s(14), false);
        let r = RECT { left: x, top: top + s(18), right: x + col_w - s(6), bottom: top + s(40) };
        text_rect(hdc, fonts.value, t.text, value, r, DT_LEFT | DT_END_ELLIPSIS);
    }

    // Процессы
    let watts = discharge_w(snap);
    text_at(hdc, fonts.title, t.text, tr.procs, left, s(lay.proc_hdr), false);
    let hint = if watts.is_some() { tr.procs_hint_w } else { tr.procs_hint_share };
    text_at(hdc, fonts.small, t.sub, hint, right, s(lay.proc_hdr), true);
    if prow.is_empty() {
        let r = RECT { left, top: s(lay.proc_top), right, bottom: s(lay.proc_top + PROC_ROW_H) };
        let msg = if v.procs.map(|p| p.unavailable).unwrap_or(false) { tr.procs_unavail } else { tr.procs_wait };
        text_rect(hdc, fonts.body, t.sub, msg, r, DT_LEFT);
    }
    for (i, r) in prow.iter().enumerate() {
        let top = s(lay.proc_top + PROC_ROW_H * i as i32);
        let iy = top + (s(PROC_ROW_H) - icon_sz) / 2;
        if r.icon != 0 {
            DrawIconEx(hdc, left, iy, r.icon as _, icon_sz, icon_sz, 0, null_mut(), DI_NORMAL);
        } else {
            let ch: String = if r.system { "…".into() } else { r.name.chars().take(1).collect::<String>().to_uppercase() };
            let rr = RECT { left, top: iy, right: left + icon_sz, bottom: iy + icon_sz };
            text_rect(hdc, fonts.small, t.sub, &ch, rr, DT_CENTER);
        }
        let row_r = |l: i32, rgt: i32| RECT { left: l, top, right: rgt, bottom: top + s(PROC_ROW_H) - s(2) };
        let vw = text_width(hdc, fonts.body_b, &r.value);
        let val_left = right - vw.max(s(48));
        text_rect(hdc, fonts.body_b, if r.system { t.sub } else { t.text }, &r.value, row_r(val_left, right), DT_RIGHT);
        let mut name_right = val_left - s(10);
        if !r.detail.is_empty() {
            let dw = text_width(hdc, fonts.small, &r.detail);
            text_rect(hdc, fonts.small, t.sub, &r.detail, row_r(val_left - s(10) - dw, val_left - s(10)), DT_RIGHT);
            name_right = val_left - s(10) - dw - s(10);
        }
        text_rect(hdc, fonts.body, if r.system { t.sub } else { t.text }, &r.name, row_r(name_x, name_right), DT_LEFT | DT_END_ELLIPSIS);
    }

    // Подвал: ссылка на отчёт за месяц и версия.
    let ver = format!("v{} · by {}", crate::VERSION, crate::AUTHOR);
    text_at(hdc, fonts.small, t.sub, &ver, right, s(lay.foot), true);
    let link = tr.monthly_link;
    let lw = text_width(hdc, fonts.body_b, link);
    text_at(hdc, fonts.body_b, t.accent, link, left, s(lay.foot), false);
    let pad = s(6);
    let vals = [left - pad, s(lay.foot) - s(16), left + lw + pad, s(lay.foot) + s(6)];
    for (a, v) in LINK.iter().zip(vals) {
        a.store(v, Ordering::Relaxed);
    }

    fonts.free();
}
