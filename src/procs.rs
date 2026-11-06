//! Нагрузка процессов на ЦП и видеокарту (счётчики производительности Windows, PDH).
//! Расход в ваттах — оценка: мощность разряда делится пропорционально нагрузке.
use std::collections::HashMap;
use std::ptr::{null, null_mut};
use std::time::Instant;

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Performance::*;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::Shell::SHDefExtractIconW;
use windows_sys::Win32::UI::WindowsAndMessaging::HICON;

use crate::util::w;

const PDH_FMT_NOCAP100: u32 = 0x8000;
/// Вес видеокарты относительно ЦП в «индексе нагрузки».
const GPU_WEIGHT: f32 = 0.5;
/// Нагрузка, эквивалентная потреблению экрана, чипсета и простоя (в % ЦП).
pub const SYSTEM_SHARE: f32 = 15.0;

#[derive(Clone, Default, Debug)]
pub struct ProcRow {
    pub key: String,
    pub display: String,
    pub icon: isize,
    pub cpu: f32,
    pub gpu: f32,
}

impl ProcRow {
    pub fn impact(&self) -> f32 {
        self.cpu + self.gpu * GPU_WEIGHT
    }
    /// Имя для показа с учётом языка (системные процессы переводятся).
    pub fn name(&self) -> String {
        crate::i18n::pretty_process(&self.key).map(|s| s.to_string()).unwrap_or_else(|| self.display.clone())
    }
}

#[derive(Clone, Default, Debug)]
pub struct ProcStats {
    pub rows: Vec<ProcRow>,
    /// Сумма нагрузки всех процессов (не только показанных).
    pub total_impact: f32,
    /// Счётчики производительности недоступны.
    pub unavailable: bool,
}

/// Средняя нагрузка программы за интервал между замерами.
#[derive(Clone, Debug)]
pub struct Group {
    pub key: String,
    pub cpu: f32,
    pub gpu: f32,
    pub pid: u32,
}

impl Group {
    pub fn impact(&self) -> f32 {
        self.cpu + self.gpu * GPU_WEIGHT
    }
}

pub enum Sample {
    /// Первый замер — счётчики только «заряжены».
    NotReady,
    Unavailable,
    Data(Vec<Group>),
}

struct Meta {
    display: String,
    path: Option<Vec<u16>>,
    icon: Option<isize>,
}

pub struct Sampler {
    query: isize,
    c_cpu: isize,
    c_pid: isize,
    c_gpu: isize,
    pub ncpu: f32,
    primed: bool,
    smooth: HashMap<String, (f32, f32)>,
    meta: HashMap<String, Meta>,
    demo: Option<Instant>,
}

unsafe fn counter_array(counter: isize, fmt: u32) -> Vec<(String, f64)> {
    let mut size: u32 = 0;
    let mut count: u32 = 0;
    let r = PdhGetFormattedCounterArrayW(counter, fmt, &mut size, &mut count, null_mut());
    if r != PDH_MORE_DATA || size == 0 {
        return Vec::new();
    }
    let mut buf = vec![0u64; (size as usize + 7) / 8];
    let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
    if PdhGetFormattedCounterArrayW(counter, fmt, &mut size, &mut count, items) != 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let it = &*items.add(i);
        if it.FmtValue.CStatus != PDH_CSTATUS_VALID_DATA && it.FmtValue.CStatus != PDH_CSTATUS_NEW_DATA {
            continue;
        }
        let mut len = 0usize;
        while *it.szName.add(len) != 0 {
            len += 1;
        }
        let name = String::from_utf16_lossy(std::slice::from_raw_parts(it.szName, len));
        out.push((name, it.FmtValue.Anonymous.doubleValue));
    }
    out
}

/// «chrome#3» → «chrome»
fn base_name(inst: &str) -> &str {
    match inst.rfind('#') {
        Some(i) if inst[i + 1..].chars().all(|c| c.is_ascii_digit()) => &inst[..i],
        _ => inst,
    }
}

unsafe fn file_description(path: &[u16]) -> Option<String> {
    let mut h: u32 = 0;
    let size = GetFileVersionInfoSizeW(path.as_ptr(), &mut h);
    if size == 0 {
        return None;
    }
    let mut data = vec![0u8; size as usize];
    if GetFileVersionInfoW(path.as_ptr(), 0, size, data.as_mut_ptr() as *mut _) == 0 {
        return None;
    }
    let mut p: *mut core::ffi::c_void = null_mut();
    let mut len: u32 = 0;
    let mut langs: Vec<(u16, u16)> = Vec::new();
    if VerQueryValueW(data.as_ptr() as *const _, w("\\VarFileInfo\\Translation").as_ptr(), &mut p, &mut len) != 0 && len >= 4 {
        let n = len as usize / 4;
        let t = p as *const u16;
        for i in 0..n {
            langs.push((*t.add(i * 2), *t.add(i * 2 + 1)));
        }
    }
    langs.push((0x0409, 0x04B0));
    langs.push((0x0409, 0x04E4));
    for (lang, cp) in langs {
        let key = w(&format!("\\StringFileInfo\\{:04x}{:04x}\\FileDescription", lang, cp));
        if VerQueryValueW(data.as_ptr() as *const _, key.as_ptr(), &mut p, &mut len) != 0 && len > 1 {
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p as *const u16, len as usize - 1));
            let s = s.trim().trim_end_matches('\0').to_string();
            if !s.is_empty() && s.len() < 60 {
                return Some(s);
            }
        }
    }
    None
}

unsafe fn resolve_meta(name: &str, pid: u32) -> Meta {
    let mut display = name.to_string();
    let mut path = None;
    if pid != 0 && crate::i18n::pretty_process(name).is_none() {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if !h.is_null() {
            let mut buf = vec![0u16; 1024];
            let mut len = buf.len() as u32;
            if QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) != 0 && len > 0 {
                buf.truncate(len as usize);
                buf.push(0);
                if let Some(d) = file_description(&buf) {
                    display = d;
                }
                path = Some(buf);
            }
            CloseHandle(h);
        }
    }
    Meta { display, path, icon: None }
}

unsafe fn extract_icon(path: &[u16]) -> isize {
    let px = ((16 * GetDpiForSystem().max(96)) as f32 / 96.0).round() as u32;
    let mut large: HICON = null_mut();
    let mut small: HICON = null_mut();
    if SHDefExtractIconW(path.as_ptr(), 0, 0, &mut large, &mut small, (px << 16) | px) == 0 {
        if !small.is_null() {
            if !large.is_null() {
                windows_sys::Win32::UI::WindowsAndMessaging::DestroyIcon(large);
            }
            return small as isize;
        }
        return large as isize;
    }
    0
}

impl Sampler {
    pub fn new(demo: bool) -> Self {
        let mut s = Sampler {
            query: 0,
            c_cpu: 0,
            c_pid: 0,
            c_gpu: 0,
            ncpu: std::thread::available_parallelism().map(|n| n.get() as f32).unwrap_or(1.0),
            primed: false,
            smooth: HashMap::new(),
            meta: HashMap::new(),
            demo: if demo { Some(Instant::now()) } else { None },
        };
        if demo {
            return s;
        }
        unsafe {
            if PdhOpenQueryW(null(), 0, &mut s.query) != 0 {
                s.query = 0;
                return s;
            }
            if PdhAddEnglishCounterW(s.query, w("\\Process(*)\\% Processor Time").as_ptr(), 0, &mut s.c_cpu) != 0 {
                s.c_cpu = 0;
            }
            if PdhAddEnglishCounterW(s.query, w("\\Process(*)\\ID Process").as_ptr(), 0, &mut s.c_pid) != 0 {
                s.c_pid = 0;
            }
            if PdhAddEnglishCounterW(s.query, w("\\GPU Engine(*)\\Utilization Percentage").as_ptr(), 0, &mut s.c_gpu) != 0 {
                s.c_gpu = 0;
            }
        }
        s
    }

    /// Имя программы для показа (кэшируется).
    pub fn display_name(&mut self, key: &str, pid: u32) -> String {
        if self.demo.is_some() {
            return key.to_string();
        }
        if !self.meta.contains_key(key) {
            let m = unsafe { resolve_meta(key, pid) };
            self.meta.insert(key.to_string(), m);
        }
        self.meta[key].display.clone()
    }

    fn icon(&mut self, key: &str) -> isize {
        let Some(m) = self.meta.get_mut(key) else { return 0 };
        if m.icon.is_none() {
            m.icon = Some(match &m.path {
                Some(p) => unsafe { extract_icon(p) },
                None => 0,
            });
        }
        m.icon.unwrap_or(0)
    }

    /// Средняя нагрузка программ с прошлого замера (без сглаживания).
    pub fn sample_raw(&mut self) -> Sample {
        if let Some(t0) = self.demo {
            return Sample::Data(demo_groups(t0.elapsed().as_secs_f32()));
        }
        if self.query == 0 || self.c_cpu == 0 {
            return Sample::Unavailable;
        }
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return Sample::NotReady;
            }
            if !self.primed {
                self.primed = true;
                return Sample::NotReady;
            }
            let cpu = counter_array(self.c_cpu, PDH_FMT_DOUBLE | PDH_FMT_NOCAP100);
            let pids: HashMap<String, u32> = if self.c_pid != 0 {
                counter_array(self.c_pid, PDH_FMT_DOUBLE).into_iter().map(|(n, v)| (n, v as u32)).collect()
            } else {
                HashMap::new()
            };

            // Видеокарта: по каждому pid — максимум по типам движков (как в диспетчере задач).
            let mut gpu_by_pid: HashMap<u32, HashMap<String, f32>> = HashMap::new();
            if self.c_gpu != 0 {
                for (inst, v) in counter_array(self.c_gpu, PDH_FMT_DOUBLE) {
                    let Some(rest) = inst.strip_prefix("pid_") else { continue };
                    let Some(pid) = rest.split('_').next().and_then(|p| p.parse::<u32>().ok()) else { continue };
                    let eng = inst.rsplit("engtype_").next().unwrap_or("").to_string();
                    *gpu_by_pid.entry(pid).or_default().entry(eng).or_insert(0.0) += v as f32;
                }
            }
            let gpu_pid: HashMap<u32, f32> = gpu_by_pid
                .into_iter()
                .map(|(pid, m)| (pid, m.values().fold(0.0f32, |a, b| a.max(*b)).min(100.0)))
                .collect();

            // Группировка по имени программы.
            let mut groups: HashMap<String, (f32, f32, u32, f32)> = HashMap::new(); // cpu, gpu, pid с макс. ЦП, этот макс.
            for (inst, v) in cpu {
                if inst == "_Total" || inst == "Idle" {
                    continue;
                }
                let base = base_name(&inst).to_string();
                let pid = pids.get(&inst).copied().unwrap_or(0);
                let c = (v as f32 / self.ncpu).max(0.0);
                let g = gpu_pid.get(&pid).copied().unwrap_or(0.0);
                let e = groups.entry(base).or_insert((0.0, 0.0, pid, -1.0));
                e.0 += c;
                e.1 += g;
                if c > e.3 {
                    e.2 = pid;
                    e.3 = c;
                }
            }
            Sample::Data(
                groups
                    .into_iter()
                    .map(|(key, (c, g, pid, _))| Group { key, cpu: c.min(100.0), gpu: g.min(100.0), pid })
                    .collect(),
            )
        }
    }

    /// Сглаженный список для окна: топ-8 с иконками.
    pub fn ui_stats(&mut self, groups: &[Group]) -> ProcStats {
        let mut cur: Vec<Group> = groups
            .iter()
            .map(|g| {
                let (c, gg) = match self.smooth.get(&g.key) {
                    Some(&(oc, og)) => (oc * 0.4 + g.cpu * 0.6, og * 0.4 + g.gpu * 0.6),
                    None => (g.cpu, g.gpu),
                };
                Group { key: g.key.clone(), cpu: c, gpu: gg, pid: g.pid }
            })
            .collect();
        self.smooth = cur.iter().map(|g| (g.key.clone(), (g.cpu, g.gpu))).collect();
        let total: f32 = cur.iter().map(|g| g.impact()).sum();
        cur.sort_by(|a, b| b.impact().partial_cmp(&a.impact()).unwrap_or(std::cmp::Ordering::Equal));
        let mut rows = Vec::new();
        for g in cur.into_iter().take(8) {
            let display = self.display_name(&g.key, g.pid);
            let icon = self.icon(&g.key);
            rows.push(ProcRow { key: g.key, display, icon, cpu: g.cpu, gpu: g.gpu });
        }
        ProcStats { rows, total_impact: total, unavailable: false }
    }
}

fn demo_groups(t: f32) -> Vec<Group> {
    let s = |k: f32| ((t * k).sin() + 1.0) / 2.0;
    let g = |key: &str, cpu: f32, gpu: f32| Group { key: key.into(), cpu, gpu, pid: 0 };
    vec![
        g("Google Chrome", 9.0 + 4.0 * s(0.9), 6.0 + 3.0 * s(1.3)),
        g("Visual Studio Code", 4.0 + 2.0 * s(0.5), 1.0),
        g("Telegram Desktop", 1.5 + s(1.1), 2.0),
        g("Хост-процесс для служб Windows", 1.6, 0.0),
        g("Antimalware Service Executable", 0.4 + 2.0 * s(0.2), 0.0),
        g("Диспетчер окон рабочего стола", 0.8, 3.0),
        g("System", 0.9, 0.0),
        g("Spotify", 0.6 + 0.5 * s(0.7), 0.5),
    ]
}
