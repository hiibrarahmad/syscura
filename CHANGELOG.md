# Changelog

## 1.0.0

The first full release. Everything from the betas, plus a real installer, built-in updates, a live process watch and much more. Changes since 0.1.0-beta.2:

### Updates
- **Built-in updates from GitHub.** Syscura checks once a day and notifies you of a new version.
  - *Settings → Updates → Download and install* downloads the Setup, checks its SHA-256 against the release's published checksums, and installs it.
  - Syscura opens again afterwards, and your history, verdicts, settings and AI key are kept.
- **Never installed twice.** Reinstalling, updating, or installing over an old portable copy always leaves exactly one Syscura and one background service. Only one copy of the app runs at a time.
- Tested on clean machines for every change: fresh install, install over itself (history kept), replacing a portable copy, and a clean uninstall.

### Install
- **Setup installer** (`Syscura-<version>-setup.exe`): installs to Program Files with a Start menu entry and an uninstaller in Settings → Apps.
- The installer sets up **background protection as a Windows service** that starts with Windows. No extra clicks.
- Updating over an older version, or over a copy in another folder, replaces it cleanly. Uninstalling removes the service and auto-start too.
- On Windows 10 without WebView2, the installer adds it.

### Tray
- **Syscura lives in the taskbar tray.** It starts quietly there after every restart. Click to open, right-click for *Open*, *Show problems* or *Quit*.
- Closing the window keeps Syscura in the tray and frees the window's memory.
- The tray tooltip shows whether protection is on and how many things need you.
- **Windows notifications for serious problems now work with the window closed.**
- Starting Syscura a second time opens the running copy instead of a new one.

### Catches what antivirus misses
- **Live process watch** (light: a process snapshot every 2 seconds, each program file checked once). It flags:
  - programs pretending to be part of Windows (for example `svchost.exe` outside System32);
  - unsigned programs running from Temp, Downloads or AppData;
  - Office and PDF programs starting PowerShell, cmd or script hosts.
- **Startup check** every 6 hours: Run keys, Startup folders and scheduled tasks. It flags unsigned programs in user folders and hidden, encoded or downloading scripts.
- New **Processes** page: every running program live, with CPU, memory, folder, publisher and signature. *Open folder*, *Scan with Defender* and *Stop* (asks first) for each.
- New actions:
  - *Scan this file or folder with Defender*;
  - *Let Defender remove the threats it found*;
  - *Defender Offline scan*, for malware that hides while Windows runs;
  - *Stop this program* (never Windows' critical processes).
- For virus detections, the affected files are listed with **Open folder** and **Delete…**. Delete warns first and moves the file to the Recycle Bin, so it can be restored.

### Remembers
- **Mark as safe** (and *This is harmful*) is saved for good: the same thing is never flagged again. The AI's verdict is saved too, but the AI can never clear a security threat; only you can.
- When a problem comes back after a fix that was confirmed to work, Syscura runs the same fix again automatically if it is safe, or offers **Fix again**.

### Every PC, fully described
- Details Windows does not report (common on laptops and pre-built PCs) are listed on the Hardware page. **Find them with AI** looks up the official specifications of that exact model (free Gemini key). Without a key, use **Search the web** / **Ask ChatGPT** and click **add** to type a value. Values from outside the PC are always marked *found online* or *added by you*.
- Problems about a file (a virus, a suspicious program) are **closed automatically once the file is gone**, whether you deleted it or Defender removed it. Each file shows *Gone ✓*.
- **Delete its folder…** removes the whole folder a threat came in, to the Recycle Bin, after a warning. Drives, Windows, Program Files and your main user folders are refused.

### Overview
- The right panel now shows Defender's state, virus-definition age, last quick and full scan, tamper protection, today's Windows errors, PC running time, free disk space and battery.
- More space between numbers and their units.
- The serious-problem banner can be closed. It comes back only for a new serious problem, and backing up is offered, not pushed.

### Fixes
- The installer no longer shows "service does not exist" or "process not found" lines on a fresh install.
- "Install as a Windows service" no longer reports a false failure: it checks what Windows actually says. It also replaces a service installed from another folder, with one admin prompt.
- Pressing Start no longer launches a second agent when protection is already on.
- The service waits briefly for a session agent that is still closing, instead of failing.
- "AI help: ·" is shown properly when no model was used yet.
- Privacy masking can no longer break on user names in other scripts (Urdu, Arabic, Turkish and others).

### More
- Settings: a *Background protection* status and an *About and help* section with links to report a problem, ask a question, check for updates, and the author's GitHub, LinkedIn and website.
- Every change is now tested on clean Windows machines (Server 2022 and 2025): hardware scan, agent start, and a full install, service check and uninstall.

## 0.1.0-beta.2

A big one: new look, free AI help, honest fix results, backups, and a hardware report that works on any PC.

### New design
- The **Verdict** design: light and dark themes, top tab bar, one plain-sentence headline per screen ("Your PC is mostly healthy. Two things need you."), Outfit and JetBrains Mono fonts, new Signal S logo and app icons.
- Problems page split into *Needs you*, *Syscura is handling*, *Fixed* and *Safe to ignore*, with a detail panel: What happened, Is it harmful?, What to do, fixes, and *What Syscura did*.
- Events page with a summary of what was read and how much is worth a look, filters and a timeline.

### Free AI help
- **No key needed:** every problem and event can open ChatGPT, Claude, Copilot, Gemini or Google AI Mode in your browser with the question written out, or search Google for the exact error.
- Optional free Gemini key: explains problems with Google Search, rates harm, and picks fixes only from Syscura's safe list. Falls back to other free models when one model's free limit is used up.
- **AI fix check:** after any fix finishes, the AI reads the result and says Fixed / Not fixed / Not sure and what to try next.
- Optional automatic mode for safe fixes, capped at 20 AI questions a day.
- Personal details (user name, PC name, folder paths, e-mail, IP addresses) are masked before anything is sent. The key is stored in Windows Credential Manager.

### Honest fixes
- Repair tools are read properly: SFC or DISM finding nothing is shown as *found nothing wrong, so it was not the cause*, and the problem stays open with a **What next** box instead of being marked fixed.
- A restore point is made before any fix that is not safe.
- Problems no longer stay stuck on "Fixing…" when the agent restarts during a fix.
- Blue screens show the decoded stop code and its usual cause (for example 0x3B, SYSTEM_SERVICE_EXCEPTION, usually a driver). Older findings get this too.
- Windows' own message is shown for every event and problem.

### Warnings and backup
- Red banner and a Windows notification for serious problems (virus found, failing drive, blue screens).
- New **Backup** page: copies your folders to another drive or USB stick. It only copies, never deletes or overwrites.

### Hardware
- The board photo, part pictures and board map were removed. Every PC is different, so Syscura now shows an accurate, detailed report read fresh at every start: exact Windows version (Windows 11 detected correctly, with version and build), firmware and Secure Boot, board and BIOS, CPU, every RAM stick, GPU, drives, monitors (real model names), battery wear, network, audio and USB. Anything not reported says *Not reported*.
- **Copy report** for support requests, and a new *Wrong hardware info* issue form.

### Under the hood
- No more online picture lookups: Syscura only goes online when you use AI or web help.
- `syscura-cli problems` shows the real result of each fix.

## 0.1.0-beta.1

First public beta.

### Problems and fixes
- Background agent (~13 MB RAM, 0% CPU when idle) that receives Windows events as they happen, filtered inside Windows.
- Knowledge base of 24 rules covering services, crashes, blue screens, hardware errors, drives, Windows Update, networking and security, each with a plain-language explanation and a harmful yes/maybe/no verdict.
- Checks the last 7 days of history on first start.
- Safe fixes run automatically (rate limited); others ask first. Every fix is verified; reversible ones can be undone. Nothing is deleted.
- Security checks: Defender detections and protection changes, new services with signature and location check, cleared event logs, suspicious PowerShell.

### Hardware
- Inventory of board, BIOS, CPU, RAM (with voltage, ranks, decoded part number), expansion slots, GPU (VBIOS, PCIe link, power limit), drives (HDD/SSD detection, health), USB devices by port.
- Live CPU load and clock from Windows, GPU sensors through NVIDIA's driver.
- Real-photo board view with exact labels for the ASUS TUF GAMING X570-PRO WIFI II; generic board map for every other board.
- Proves which slot the GPU and NVMe SSD sit in from their PCIe paths.
- Part pictures found online for free and cached; you can set your own.

### App
- Overview, Problems, Hardware and Events screens. Start protection and install-as-service from the app.
- Command-line client `syscura-cli`.
