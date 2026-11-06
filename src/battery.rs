//! Чтение батареи через драйвер Windows (IOCTL_BATTERY_*), как это делает сама система.
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Power::*;
use windows_sys::Win32::System::IO::DeviceIoControl;

#[derive(Clone, Default, Debug)]
pub struct Snapshot {
    pub present: bool,
    pub on_ac: bool,
    pub charging: bool,
    pub discharging: bool,
    pub percent: Option<f32>,
    /// Вт, со знаком: + заряд, − разряд.
    pub power_w: Option<f32>,
    pub voltage_v: Option<f32>,
    pub current_a: Option<f32>,
    pub energy_wh: Option<f32>,
    pub full_wh: Option<f32>,
    pub design_wh: Option<f32>,
    pub cycles: Option<u32>,
    pub temp_c: Option<f32>,
    pub chemistry: String,
    pub name: String,
    pub manufacturer: String,
    /// До разряда (если разряжается) или до полного заряда (если заряжается).
    pub time_left_min: Option<u32>,
}

impl Snapshot {
    pub fn health(&self) -> Option<f32> {
        match (self.full_wh, self.design_wh) {
            (Some(f), Some(d)) if d > 0.0 => Some((f / d * 100.0).min(100.0)),
            _ => None,
        }
    }
}

struct One {
    relative: bool,
    capacity: Option<u32>,
    full: Option<u32>,
    design: Option<u32>,
    voltage: Option<u32>,
    rate: Option<i32>,
    state: u32,
    cycles: u32,
    temp_dk: Option<u32>,
    chemistry: String,
    name: String,
    manufacturer: String,
}

pub struct Reader {
    paths: Vec<Vec<u16>>,
    last_enum: Option<Instant>,
    demo: Option<Instant>,
}

impl Reader {
    pub fn new(demo: bool) -> Self {
        Reader {
            paths: Vec::new(),
            last_enum: None,
            demo: if demo { Some(Instant::now()) } else { None },
        }
    }

    fn enumerate(&mut self) {
        self.paths.clear();
        self.last_enum = Some(Instant::now());
        unsafe {
            let hdev = SetupDiGetClassDevsW(
                &GUID_DEVCLASS_BATTERY,
                null(),
                null_mut(),
                DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
            );
            if hdev == -1 || hdev == 0 {
                return;
            }
            for idx in 0..8u32 {
                let mut did: SP_DEVICE_INTERFACE_DATA = zeroed();
                did.cbSize = size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
                if SetupDiEnumDeviceInterfaces(hdev, null(), &GUID_DEVCLASS_BATTERY, idx, &mut did)
                    == 0
                {
                    break;
                }
                let mut req: u32 = 0;
                SetupDiGetDeviceInterfaceDetailW(hdev, &did, null_mut(), 0, &mut req, null_mut());
                if req == 0 {
                    continue;
                }
                let mut buf = vec![0u64; (req as usize + 7) / 8 + 1];
                let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
                (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
                if SetupDiGetDeviceInterfaceDetailW(hdev, &did, detail, req, &mut req, null_mut())
                    != 0
                {
                    let p = (*detail).DevicePath.as_ptr();
                    let mut len = 0usize;
                    while *p.add(len) != 0 {
                        len += 1;
                    }
                    let mut v = std::slice::from_raw_parts(p, len).to_vec();
                    v.push(0);
                    self.paths.push(v);
                }
            }
            SetupDiDestroyDeviceInfoList(hdev);
        }
    }

    pub fn read(&mut self) -> Snapshot {
        if let Some(t0) = self.demo {
            return demo_snapshot(t0.elapsed().as_secs_f32());
        }
        let stale = self
            .last_enum
            .map(|t| t.elapsed() > Duration::from_secs(60))
            .unwrap_or(true);
        if self.paths.is_empty() || stale {
            self.enumerate();
        }
        let mut list: Vec<One> = self.paths.iter().filter_map(|p| unsafe { read_one(p) }).collect();
        if list.is_empty() && !self.paths.is_empty() {
            // Батарею могли переподключить — перечитать список устройств.
            self.enumerate();
            list = self.paths.iter().filter_map(|p| unsafe { read_one(p) }).collect();
        }

        let mut sps: SYSTEM_POWER_STATUS = unsafe { zeroed() };
        let have_sps = unsafe { GetSystemPowerStatus(&mut sps) } != 0;

        let mut s = Snapshot::default();
        s.on_ac = have_sps && sps.ACLineStatus == 1;

        if list.is_empty() {
            // Нет доступа к драйверу — хотя бы процент из системы.
            if have_sps && sps.BatteryFlag != 128 && sps.BatteryLifePercent <= 100 {
                s.present = true;
                s.percent = Some(sps.BatteryLifePercent as f32);
                s.charging = sps.BatteryFlag & 8 != 0;
                s.discharging = !s.on_ac;
            }
            return s;
        }

        s.present = true;
        let mut cap_sum = 0u64;
        let mut full_sum = 0u64;
        let mut design_sum = 0u64;
        let mut cap_ok = true;
        let mut full_ok = true;
        let mut design_ok = true;
        let mut rate_sum: i64 = 0;
        let mut rate_ok = false;
        let mut relative = false;

        for b in &list {
            relative |= b.relative;
            match b.capacity {
                Some(c) => cap_sum += c as u64,
                None => cap_ok = false,
            }
            match b.full {
                Some(c) if c > 0 => full_sum += c as u64,
                _ => full_ok = false,
            }
            match b.design {
                Some(c) if c > 0 => design_sum += c as u64,
                _ => design_ok = false,
            }
            if b.state & BATTERY_CHARGING != 0 {
                s.charging = true;
            }
            if b.state & BATTERY_DISCHARGING != 0 {
                s.discharging = true;
            }
            if b.state & BATTERY_POWER_ON_LINE != 0 {
                s.on_ac = true;
            }
            if let Some(r) = b.rate {
                rate_ok = true;
                let r = r as i64;
                // Знак берём из состояния: некоторые драйверы отдают модуль.
                rate_sum += if b.state & BATTERY_DISCHARGING != 0 {
                    -r.abs()
                } else if b.state & BATTERY_CHARGING != 0 {
                    r.abs()
                } else {
                    r
                };
            }
        }

        let first = &list[0];
        s.chemistry = first.chemistry.clone();
        s.name = first.name.clone();
        s.manufacturer = first.manufacturer.clone();
        s.cycles = if first.cycles > 0 { Some(first.cycles) } else { None };
        s.temp_c = first
            .temp_dk
            .map(|t| t as f32 / 10.0 - 273.15)
            .filter(|c| (-20.0..=100.0).contains(c));
        s.voltage_v = first.voltage.map(|v| v as f32 / 1000.0);

        if relative {
            if cap_ok && full_ok && full_sum > 0 {
                s.percent = Some((cap_sum as f32 / full_sum as f32 * 100.0).min(100.0));
            }
        } else {
            if cap_ok {
                s.energy_wh = Some(cap_sum as f32 / 1000.0);
            }
            if full_ok {
                s.full_wh = Some(full_sum as f32 / 1000.0);
            }
            if design_ok {
                s.design_wh = Some(design_sum as f32 / 1000.0);
            }
            if let (Some(e), Some(f)) = (s.energy_wh, s.full_wh) {
                s.percent = Some((e / f * 100.0).clamp(0.0, 100.0));
            }
            if rate_ok {
                let p = rate_sum as f32 / 1000.0;
                s.power_w = Some(p);
                if let Some(v) = s.voltage_v.filter(|v| *v > 1.0) {
                    s.current_a = Some(p.abs() / v);
                }
                if p.abs() > 0.05 {
                    if let (Some(e), Some(f)) = (s.energy_wh, s.full_wh) {
                        let hours = if p < 0.0 { e / -p } else { (f - e).max(0.0) / p };
                        if hours.is_finite() && hours < 100.0 {
                            s.time_left_min = Some((hours * 60.0).round() as u32);
                        }
                    }
                }
            }
        }
        if s.percent.is_none() && have_sps && sps.BatteryLifePercent <= 100 {
            s.percent = Some(sps.BatteryLifePercent as f32);
        }
        s
    }
}

unsafe fn ioctl<I, O>(h: HANDLE, code: u32, input: &I, out: &mut O) -> bool {
    let mut ret: u32 = 0;
    DeviceIoControl(
        h,
        code,
        input as *const I as *const _,
        size_of::<I>() as u32,
        out as *mut O as *mut _,
        size_of::<O>() as u32,
        &mut ret,
        null_mut(),
    ) != 0
}

unsafe fn query_string(h: HANDLE, tag: u32, level: BATTERY_QUERY_INFORMATION_LEVEL) -> String {
    let q = BATTERY_QUERY_INFORMATION { BatteryTag: tag, InformationLevel: level, AtRate: 0 };
    let mut buf = [0u16; 128];
    if ioctl(h, IOCTL_BATTERY_QUERY_INFORMATION, &q, &mut buf) {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len]).trim().to_string()
    } else {
        String::new()
    }
}

unsafe fn read_one(path: &[u16]) -> Option<One> {
    let h = CreateFileW(
        path.as_ptr(),
        GENERIC_READ | GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        null(),
        OPEN_EXISTING,
        FILE_ATTRIBUTE_NORMAL,
        null_mut(),
    );
    if h == INVALID_HANDLE_VALUE {
        return None;
    }
    let res = (|| {
        let wait: u32 = 0;
        let mut tag: u32 = 0;
        if !ioctl(h, IOCTL_BATTERY_QUERY_TAG, &wait, &mut tag) || tag == 0 {
            return None;
        }
        let q = BATTERY_QUERY_INFORMATION { BatteryTag: tag, InformationLevel: BatteryInformation, AtRate: 0 };
        let mut info: BATTERY_INFORMATION = zeroed();
        if !ioctl(h, IOCTL_BATTERY_QUERY_INFORMATION, &q, &mut info) {
            return None;
        }
        let ws = BATTERY_WAIT_STATUS { BatteryTag: tag, Timeout: 0, PowerState: 0, LowCapacity: 0, HighCapacity: 0 };
        let mut st: BATTERY_STATUS = zeroed();
        if !ioctl(h, IOCTL_BATTERY_QUERY_STATUS, &ws, &mut st) {
            return None;
        }
        let qt = BATTERY_QUERY_INFORMATION { BatteryTag: tag, InformationLevel: BatteryTemperature, AtRate: 0 };
        let mut temp: u32 = 0;
        let temp_dk = if ioctl(h, IOCTL_BATTERY_QUERY_INFORMATION, &qt, &mut temp) && temp > 0 {
            Some(temp)
        } else {
            None
        };
        let unk = |v: u32| if v == BATTERY_UNKNOWN_CAPACITY { None } else { Some(v) };
        let chem: String = info
            .Chemistry
            .iter()
            .filter(|c| c.is_ascii_graphic())
            .map(|&c| c as char)
            .collect();
        Some(One {
            relative: info.Capabilities & BATTERY_CAPACITY_RELATIVE != 0,
            capacity: unk(st.Capacity),
            full: unk(info.FullChargedCapacity),
            design: unk(info.DesignedCapacity),
            voltage: if st.Voltage == BATTERY_UNKNOWN_VOLTAGE || st.Voltage == 0 { None } else { Some(st.Voltage) },
            rate: if st.Rate == BATTERY_UNKNOWN_RATE as i32 { None } else { Some(st.Rate) },
            state: st.PowerState,
            cycles: info.CycleCount,
            temp_dk,
            chemistry: chem,
            name: query_string(h, tag, BatteryDeviceName),
            manufacturer: query_string(h, tag, BatteryManufactureName),
        })
    })();
    CloseHandle(h);
    res
}

/// Синтетические данные для проверки интерфейса (`--demo`).
fn demo_snapshot(t: f32) -> Snapshot {
    let phase = (t / 40.0) as i32 % 3;
    let wobble = (t * 0.7).sin() * 3.0 + (t * 2.3).sin() * 1.2;
    let (charging, discharging, p) = match phase {
        0 => (false, true, -(11.0 + wobble).max(2.0)),
        1 => (true, false, 38.0 + wobble * 2.0),
        _ => (false, true, -(24.0 + wobble * 1.5)),
    };
    let v = 11.4 + (t * 0.05).sin() * 0.3;
    let full = 46.2;
    let e = 32.7;
    Snapshot {
        present: true,
        on_ac: charging,
        charging,
        discharging,
        percent: Some(e / full * 100.0),
        power_w: Some(p),
        voltage_v: Some(v),
        current_a: Some(p.abs() / v),
        energy_wh: Some(e),
        full_wh: Some(full),
        design_wh: Some(48.96),
        cycles: Some(312),
        temp_c: Some(31.4),
        chemistry: "LION".into(),
        name: "NH50DB-BAT".into(),
        manufacturer: "Notebook".into(),
        time_left_min: Some(if p < 0.0 { (e / -p * 60.0) as u32 } else { ((full - e) / p * 60.0) as u32 }),
    }
}
