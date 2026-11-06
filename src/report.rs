//! HTML-отчёт о расходе батареи за месяц.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::i18n::{code, fill, lang, pretty_process, t, Lang};
use crate::usage::{self, Month, OTHER};
use crate::util::{f1, fmt_duration, local_now};

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        _ => 28,
    }
}

pub fn month_title(ym: &str) -> String {
    let m: usize = ym[5..7].parse().unwrap_or(1);
    format!("{} {}", t().months[(m - 1).min(11)], &ym[..4])
}

fn nice_max(v: f64) -> f64 {
    for s in [1.0, 2.0, 5.0, 10.0, 20.0, 30.0, 50.0, 75.0, 100.0, 150.0, 200.0, 300.0, 500.0, 1000.0] {
        if v <= s {
            return s;
        }
    }
    (v / 1000.0).ceil() * 1000.0
}

fn day_chart(ym: &str, data: &Month) -> String {
    let y: i32 = ym[..4].parse().unwrap_or(2026);
    let m: u32 = ym[5..7].parse().unwrap_or(1);
    let dim = days_in_month(y, m);
    let vals: Vec<f64> = (1..=dim)
        .map(|d| data.get(&format!("{}-{:02}", ym, d)).map(|x| x.battery_wh).unwrap_or(0.0))
        .collect();
    let maxv = nice_max(vals.iter().cloned().fold(0.0, f64::max).max(0.5));
    let (w, h, left, top, bottom) = (820.0, 200.0, 44.0, 12.0, 26.0);
    let ph = h - top - bottom;
    let slot = (w - left) / dim as f64;
    let bw = (slot * 0.62).max(3.0);
    let mut s = format!(r##"<svg viewBox="0 0 {w} {h}" class="chart" role="img"><defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#7c5cff"/><stop offset="1" stop-color="#2f7cf6"/></linearGradient></defs>"##);
    for i in 0..=3 {
        let v = maxv * i as f64 / 3.0;
        let yy = top + ph - ph * i as f64 / 3.0;
        s += &format!(r#"<line x1="{left}" x2="{w}" y1="{yy:.1}" y2="{yy:.1}" class="grid"/><text x="{:.1}" y="{:.1}" class="axis" text-anchor="end">{}</text>"#, left - 8.0, yy + 4.0, esc(&crate::util::dec(format!("{}", (v * 10.0).round() / 10.0))));
    }
    for (i, v) in vals.iter().enumerate() {
        let x = left + slot * i as f64 + (slot - bw) / 2.0;
        let bh = if *v > 0.0 { (ph * v / maxv).max(2.0) } else { 0.0 };
        if bh > 0.0 {
            s += &format!(r#"<rect x="{x:.1}" y="{:.1}" width="{bw:.1}" height="{bh:.1}" rx="{:.1}" fill="url(#g)"><title>{} — {} {}</title></rect>"#, top + ph - bh, (bw / 2.0).min(4.0), i + 1, esc(&f1(*v as f32)), t().u_wh);
        }
        let day = i as u32 + 1;
        if day == 1 || day % 5 == 0 || day == dim {
            s += &format!(r#"<text x="{:.1}" y="{:.1}" class="axis" text-anchor="middle">{}</text>"#, x + bw / 2.0, h - 6.0, day);
        }
    }
    s += "</svg>";
    s
}

pub fn render(ym: &str, data: &Month, full_wh: Option<f32>, months: &[String]) -> String {
    let tr = t();
    let total_wh: f64 = data.values().map(|d| d.battery_wh).sum();
    let total_s: f64 = data.values().map(|d| d.battery_s).sum();

    // Сводка по программам за месяц.
    let mut apps: HashMap<String, (String, f64, f64)> = HashMap::new();
    for d in data.values() {
        for (k, a) in &d.apps {
            let e = apps.entry(k.clone()).or_insert((String::new(), 0.0, 0.0));
            if !a.name.is_empty() {
                e.0 = a.name.clone();
            }
            e.1 += a.wh;
            e.2 += a.cpu_s;
        }
    }
    let apps_total: f64 = apps.values().map(|a| a.1).sum::<f64>().max(1e-9);
    let mut list: Vec<(String, String, f64, f64)> = apps
        .into_iter()
        .map(|(k, (n, wh, cpu))| {
            let name = if k == OTHER {
                tr.other_bucket.to_string()
            } else if let Some(p) = pretty_process(&k) {
                p.to_string()
            } else if n.is_empty() {
                k.clone()
            } else {
                n
            };
            (k, name, wh, cpu)
        })
        .collect();
    list.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    const TOP: usize = 15;
    let mut rows = String::new();
    let max_wh = list.iter().filter(|r| r.0 != OTHER).map(|r| r.2).fold(0.0, f64::max).max(1e-9);
    let mut shown = 0usize;
    let (mut rest_n, mut rest_wh, mut rest_cpu) = (0usize, 0.0, 0.0);
    let mut other_row = String::new();
    for (k, name, wh, cpu) in &list {
        let share = wh / apps_total * 100.0;
        let row = |cls: &str, name: &str, wh: f64, cpu: f64, bar: bool| {
            let barw = if bar { (wh / max_wh * 100.0).clamp(1.0, 100.0) } else { 0.0 };
            format!(
                r#"<tr class="{cls}"><td><div class="name">{}</div>{}</td><td class="num">{} {}</td><td class="num">{}%</td><td class="num">{}</td></tr>"#,
                esc(name),
                if bar { format!(r#"<div class="bar"><span style="width:{barw:.1}%"></span></div>"#) } else { String::new() },
                esc(&crate::util::dec(format!("{:.1}", wh))),
                tr.u_wh,
                esc(&crate::util::dec(format!("{:.1}", wh / apps_total * 100.0))),
                if cpu > 0.0 { esc(&fmt_duration((cpu / 60.0).round() as u32)) } else { "—".into() },
            )
        };
        if k == OTHER {
            other_row = row("other", name, *wh, 0.0, false);
            continue;
        }
        if shown < TOP && share >= 0.5 {
            rows += &row("", name, *wh, *cpu, true);
            shown += 1;
        } else {
            rest_n += 1;
            rest_wh += wh;
            rest_cpu += cpu;
        }
    }
    if rest_n > 0 {
        let label = fill(tr.r_others, &[&rest_n.to_string()]);
        rows += &format!(
            r#"<tr class="other"><td><div class="name">{}</div></td><td class="num">{} {}</td><td class="num">{}%</td><td class="num">{}</td></tr>"#,
            esc(&label),
            esc(&crate::util::dec(format!("{:.1}", rest_wh))),
            tr.u_wh,
            esc(&crate::util::dec(format!("{:.1}", rest_wh / apps_total * 100.0))),
            esc(&fmt_duration((rest_cpu / 60.0).round() as u32)),
        );
    }
    rows += &other_row;

    let mut cards = String::new();
    let card = |v: String, l: &str| format!(r#"<div class="card"><div class="v">{}</div><div class="l">{}</div></div>"#, v, esc(l));
    cards += &card(format!("{} <small>{}</small>", esc(&f1(total_wh as f32)), tr.u_wh), tr.r_used);
    cards += &card(esc(&fmt_duration((total_s / 60.0).round() as u32)), tr.r_time);
    if total_s > 60.0 {
        cards += &card(format!("{} <small>{}</small>", esc(&f1((total_wh / (total_s / 3600.0)) as f32)), tr.u_w), tr.r_avg);
    }
    if let Some(f) = full_wh.filter(|f| *f > 1.0) {
        cards += &card(esc(&f1((total_wh / f as f64) as f32)), tr.r_charges);
    }

    let nav: String = months
        .iter()
        .map(|m| {
            let cls = if m == ym { "pill on" } else { "pill" };
            format!(r#"<a class="{cls}" href="report-{m}.html">{}</a>"#, esc(&month_title(m)))
        })
        .collect();

    let (y, mo, d, hh, mm) = local_now();
    let stamp = if lang() == Lang::En {
        format!("{:04}-{:02}-{:02} {:02}:{:02}", y, mo, d, hh, mm)
    } else {
        format!("{:02}.{:02}.{:04} {:02}:{:02}", d, mo, y, hh, mm)
    };

    let body = if total_s < 1.0 {
        format!(r#"<p class="empty">{}</p>"#, esc(tr.r_empty))
    } else {
        format!(
            r#"<div class="cards">{cards}</div>
<h2>{}</h2>{}
<h2>{}</h2>
<table><thead><tr><th>{}</th><th class="num">{}</th><th class="num">{}</th><th class="num">{}</th></tr></thead><tbody>{rows}</tbody></table>"#,
            esc(tr.r_by_day),
            day_chart(ym, data),
            esc(tr.r_apps),
            esc(tr.r_col_app),
            esc(tr.r_col_energy),
            esc(tr.r_col_share),
            esc(tr.r_col_cpu),
        )
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="{lang}"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>BatteryStats — {title}</title>
<style>
:root{{--bg:#fff;--text:#1d1d22;--sub:#76767e;--sep:#eeeef2;--panel:#f6f7fa;--accent:#2f7cf6;--violet:#7c5cff}}
@media (prefers-color-scheme:dark){{:root{{--bg:#1b1b1e;--text:#f2f2f5;--sub:#9a9aa2;--sep:#2e2e33;--panel:#26262b;--accent:#5aa2ff;--violet:#9b85ff}}}}
*{{box-sizing:border-box}}
html,body{{margin:0;background:var(--bg);color:var(--text)}}
body{{font:15px/1.5 "Segoe UI Variable Text","Segoe UI",system-ui,-apple-system,sans-serif;min-height:100vh;
background:radial-gradient(600px 420px at 100% 0,rgba(47,124,246,.10),transparent 70%),radial-gradient(520px 420px at 0 100%,rgba(124,92,255,.08),transparent 70%) fixed}}
.wrap{{max-width:900px;margin:0 auto;padding:56px 40px 72px}}
.eyebrow{{color:var(--accent);font-size:12px;font-weight:600;letter-spacing:.08em;text-transform:uppercase}}
h1{{font:600 36px/1.15 "Segoe UI Variable Display","Segoe UI",system-ui,sans-serif;margin:8px 0 6px;letter-spacing:-.01em}}
.month{{color:var(--sub);font-size:18px}}
.nav{{display:flex;flex-wrap:wrap;margin:28px 0 8px}}
.pill{{margin:0 8px 8px 0;padding:5px 12px;border-radius:999px;background:var(--panel);color:var(--sub);text-decoration:none;font-size:13px}}
.pill.on{{background:var(--accent);color:#fff}}
.cards{{display:flex;flex-wrap:wrap;margin:40px 0 8px}}
.card{{margin:0 48px 16px 0}}
.card .v{{font:600 30px/1.2 "Segoe UI Variable Display","Segoe UI",system-ui,sans-serif}}
.card .v small{{font-size:16px;color:var(--sub);font-weight:500}}
.card .l{{color:var(--sub);font-size:13px;margin-top:2px}}
h2{{font-size:17px;font-weight:600;margin:52px 0 14px}}
.chart{{width:100%;height:auto;display:block}}
.chart .grid{{stroke:var(--sep);stroke-width:1}}
.chart .axis{{fill:var(--sub);font-size:11px}}
table{{width:100%;border-collapse:collapse}}
th{{text-align:left;font-size:12px;font-weight:500;color:var(--sub);padding:0 0 10px;border-bottom:1px solid var(--sep)}}
td{{padding:11px 0;border-bottom:1px solid var(--sep);vertical-align:top}}
.num{{text-align:right;white-space:nowrap;padding-left:20px;font-variant-numeric:tabular-nums}}
td.num{{font-weight:600}}
tr.other td{{color:var(--sub);font-weight:400}}
.name{{overflow:hidden;text-overflow:ellipsis}}
.bar{{height:4px;border-radius:2px;background:var(--panel);margin-top:7px;max-width:420px}}
.bar span{{display:block;height:100%;border-radius:2px;background:linear-gradient(90deg,var(--accent),var(--violet))}}
.note{{color:var(--sub);font-size:13px;margin-top:40px;max-width:640px}}
.empty{{color:var(--sub);margin:44px 0}}
.foot{{color:var(--sub);font-size:12px;margin-top:14px}}
</style></head><body><div class="wrap">
<div class="eyebrow">{eyebrow}</div>
<h1>{h1}</h1>
<div class="month">{title}</div>
<div class="nav">{nav}</div>
{body}
<p class="note">{note}</p>
<div class="foot">{generated} · BatteryStats {ver} · by {author}</div>
</div></body></html>"#,
        lang = code(lang()),
        title = esc(&month_title(ym)),
        eyebrow = esc(tr.r_eyebrow),
        h1 = esc(tr.r_title),
        note = esc(tr.r_note),
        generated = esc(&fill(tr.r_generated, &[&stamp])),
        ver = crate::VERSION,
        author = crate::AUTHOR,
    )
}

/// Сформировать отчёты за все месяцы и вернуть путь к отчёту за `ym`.
pub fn write_all(dir: &Path, ym: &str, current: &str, full_wh: Option<f32>) -> std::io::Result<PathBuf> {
    let months = usage::months_available(dir, current);
    let out_dir = dir.join("reports");
    std::fs::create_dir_all(&out_dir)?;
    let mut target = out_dir.join(format!("report-{}.html", ym));
    for m in &months {
        let data = usage::read_month(dir, m);
        let html = render(m, &data, full_wh, &months);
        let p = out_dir.join(format!("report-{}.html", m));
        std::fs::write(&p, html)?;
        if m == ym {
            target = p;
        }
    }
    Ok(target)
}
