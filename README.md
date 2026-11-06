<p align="center">
  <img src="docs/icon.png" width="96" alt="BatteryStats">
</p>

<h1 align="center">BatteryStats</h1>

<p align="center">
  Live battery watts in your Windows tray and the apps that drain the battery.<br>
  Battery monitor for Windows 10 and 11 · by alikken
</p>

<p align="center">
  <img alt="version" src="https://img.shields.io/badge/version-1.2.1-2f7cf6">
  <img alt="platform" src="https://img.shields.io/badge/Windows-10%20%7C%2011-7c5cff">
  <img alt="license" src="https://img.shields.io/badge/license-MIT-22a55b">
</p>

<p align="center">
  <img src="docs/screenshots/hero.png" alt="BatteryStats window in light and dark themes" width="820">
</p>

BatteryStats shows your battery's power draw in watts inside the tray icon, like Stats on macOS. Click the icon and a window opens with a live chart, battery details and the apps that use the most power. While your laptop runs on battery, BatteryStats logs usage and builds a monthly report of the energy you spent and the apps that took it.

The app weighs about 650 KB, needs no admin rights and makes no network requests.

## Features

- **Watts in the tray.** The number updates every second. White means discharging, green means charging, blue means plugged in. You can switch the icon to charge percent.
- **Details window.** Power, charge, time to empty or to full, a 5-minute chart, voltage, current, energy left, health, cycle count and temperature when your battery reports it.
- **Apps.** CPU and GPU load per app with an estimate of its power draw in watts.
- **Monthly report.** Energy used in Wh, time on battery, average power, a per-day chart, top apps and earlier months.
- **Three languages.** English, Russian and Polish. BatteryStats follows your Windows language, and you can switch it from the tray menu.
- **Light and dark themes** with sharp rendering at any display scale.
- **Installer** in the same three languages. It installs without a UAC prompt, and you remove it from Settings → Apps.

## Screenshots

| Monthly report | Tray icons |
|---|---|
| <img src="docs/screenshots/report.png" width="420"> | <img src="docs/screenshots/tray-icons.png" width="300"><br><br>Discharging · charging · plugged in · under 10 W with one decimal.<br>Dark taskbar on top, light taskbar below. |

<p align="center"><img src="docs/screenshots/installer.png" width="560" alt="Installer"></p>

The screenshots come from demo mode with test data.

## Install

1. Download `BatteryStats-Setup-1.2.1.exe` from [Releases](../../releases). You can also take it from the [`release/`](release) folder.
2. Run the installer. You don't need admin rights: it puts BatteryStats in `%LOCALAPPDATA%\Programs\BatteryStats`.
3. Find the icon next to the clock. If you can't see it, click the `^` arrow and drag the icon onto the taskbar.

The installer has no code signature yet. If Windows shows "Windows protected your PC", click **More info**, then **Run anyway**.

**Requirements:** Windows 10 or 11, 64-bit, a laptop with a battery.

## License

[MIT](LICENSE) © 2026 alikken
