//! Помесячная статистика расхода батареи по программам.
//! Файлы: %LOCALAPPDATA%\BatteryStats\usage-ГГГГ-ММ.tsv
//!   D <tab> дата <tab> секунд_от_батареи <tab> Вт·ч
//!   A <tab> дата <tab> ключ <tab> Вт·ч <tab> ЦП_секунд <tab> имя
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Ключ корзины «экран, система и прочее».
pub const OTHER: &str = "~other";

#[derive(Clone, Default, Debug)]
pub struct AppUse {
    pub name: String,
    pub wh: f64,
    pub cpu_s: f64,
}

#[derive(Clone, Default, Debug)]
pub struct Day {
    pub battery_s: f64,
    pub battery_wh: f64,
    pub apps: HashMap<String, AppUse>,
}

pub type Month = BTreeMap<String, Day>;

pub struct Usage {
    days: Month,
    dirty: bool,
    last_save: Instant,
}

fn clean(s: &str) -> String {
    s.chars().map(|c| if c == '\t' || c == '\n' || c == '\r' { ' ' } else { c }).collect()
}

pub fn month_path(dir: &Path, ym: &str) -> PathBuf {
    dir.join(format!("usage-{}.tsv", ym))
}

pub fn read_month(dir: &Path, ym: &str) -> Month {
    let mut m = Month::new();
    let Ok(text) = std::fs::read_to_string(month_path(dir, ym)) else { return m };
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.first().copied() {
            Some("D") if f.len() >= 4 => {
                let d = m.entry(f[1].to_string()).or_default();
                d.battery_s += f[2].parse::<f64>().unwrap_or(0.0);
                d.battery_wh += f[3].parse::<f64>().unwrap_or(0.0);
            }
            Some("A") if f.len() >= 6 => {
                let d = m.entry(f[1].to_string()).or_default();
                let a = d.apps.entry(f[2].to_string()).or_default();
                a.wh += f[3].parse::<f64>().unwrap_or(0.0);
                a.cpu_s += f[4].parse::<f64>().unwrap_or(0.0);
                if a.name.is_empty() {
                    a.name = f[5..].join(" ");
                }
            }
            _ => {}
        }
    }
    m
}

fn write_month(dir: &Path, ym: &str, days: &Month) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut out = String::from("#BatteryStats usage v1\n");
    for (date, d) in days.iter().filter(|(k, _)| k.starts_with(ym)) {
        out += &format!("D\t{}\t{:.0}\t{:.4}\n", date, d.battery_s, d.battery_wh);
        let mut apps: Vec<_> = d.apps.iter().collect();
        apps.sort_by(|a, b| b.1.wh.partial_cmp(&a.1.wh).unwrap_or(std::cmp::Ordering::Equal));
        for (key, a) in apps {
            out += &format!("A\t{}\t{}\t{:.5}\t{:.1}\t{}\n", date, clean(key), a.wh, a.cpu_s, clean(&a.name));
        }
    }
    let path = month_path(dir, ym);
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, out)?;
    std::fs::rename(&tmp, &path)
}

/// Доступные месяцы ("ГГГГ-ММ"), новые первыми; текущий есть всегда.
pub fn months_available(dir: &Path, current: &str) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| {
                    let n = e.file_name().to_string_lossy().to_string();
                    n.strip_prefix("usage-").and_then(|r| r.strip_suffix(".tsv")).map(|s| s.to_string())
                })
                .filter(|s| s.len() == 7)
                .collect()
        })
        .unwrap_or_default();
    if !v.iter().any(|m| m == current) {
        v.push(current.to_string());
    }
    v.sort();
    v.dedup();
    v.reverse();
    v
}

impl Usage {
    /// Загружает текущий месяц, чтобы продолжить его копить.
    pub fn load(dir: &Path, today: &str) -> Usage {
        Usage { days: read_month(dir, &today[..7]), dirty: false, last_save: Instant::now() }
    }

    /// Добавить интервал работы от батареи. `parts` — (ключ, имя, Вт·ч, ЦП-секунд).
    pub fn add(&mut self, date: &str, battery_wh: f64, battery_s: f64, parts: &[(String, String, f64, f64)]) {
        let d = self.days.entry(date.to_string()).or_default();
        d.battery_wh += battery_wh;
        d.battery_s += battery_s;
        for (key, name, wh, cpu) in parts {
            let a = d.apps.entry(key.clone()).or_default();
            a.wh += wh;
            a.cpu_s += cpu;
            if !name.is_empty() && name != key {
                a.name = name.clone();
            }
        }
        self.dirty = true;
    }

    pub fn needs_save(&self) -> bool {
        self.dirty && self.last_save.elapsed().as_secs() >= 300
    }

    /// Записать на диск все месяцы из памяти; оставить в памяти только текущий.
    pub fn save(&mut self, dir: &Path, today: &str) {
        if !self.dirty {
            return;
        }
        let mut months: Vec<String> = self.days.keys().map(|k| k[..7].to_string()).collect();
        months.dedup();
        let mut ok = true;
        for ym in &months {
            if write_month(dir, ym, &self.days).is_err() {
                ok = false;
            }
        }
        if ok {
            let cur = &today[..7];
            self.days.retain(|k, _| k.starts_with(cur));
            self.dirty = false;
        }
        self.last_save = Instant::now();
    }
}
