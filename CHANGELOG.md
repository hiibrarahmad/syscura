# Changelog

## 1.1.0

Security hardening, a Security page, deeper malware checks, and Windows on ARM. Changes since 1.0.0:

### Safer by design
- **Only Syscura can change things through its service.** Before, any program running on the PC could ask the background service (which runs with full system rights) to mark something as safe, ignore a problem, or disable another program's service. Now anyone may still read, but changes are only accepted from Syscura's own app and command line next to the service, or from a program you ran as administrator.
- **Signed updates.** Every release's checksum list is signed with Syscura's own key, and the app checks that signature before it installs an update. A copy of the release files on GitHub is no longer enough to push an update.
- **Portable copies are moved to Program Files before becoming a service**, because a service running with full rights must not live in a folder any program can change. The app then opens from there.
- **The database repairs itself.** It is checked at every start and backed up daily (the last 3 are kept). A damaged database (after a power cut, for example) is set aside, never deleted, and Syscura continues from the last good backup.
- Releases are now built and tested by GitHub Actions, never on a PC, ready for free code signing (SignPath Foundation, applied for). Dependencies are checked for known security holes on every change, and the parsers that read event logs, settings files and AI replies are tested with hundreds of random inputs.

### Security page (new)
- **A security score out of 100** from Windows' own settings: firewall, Defender real-time protection, virus definitions, tamper protection, User Account Control, recent Windows updates, SMBv1, Remote Desktop, Secure Boot, Microsoft's vulnerable driver blocklist, BitLocker, LSA protection and Controlled folder access. Each check says what it found and what to do.
- One-click fixes where there is a safe one: turn the firewall on, turn Remote Desktop off, remove SMBv1, turn on the vulnerable driver blocklist, turn on Controlled folder access. Changes that can be undone get an Undo.
- **Programs with updates:** lists what winget can update and updates one or all of them in a window you can watch.
- **Ransomware tripwire** (optional): a hidden file in Documents and Pictures, checked every 5 seconds. If something encrypts or renames it, Syscura warns at once.

### Deeper malware checks
- Startup tricks: debugger hijacks (including the sign-in-screen backdoor on sethc.exe and utilman.exe), a replaced Windows shell or Userinit, AppInit DLLs, WMI tasks that run commands, and unsigned services in user folders.
- Network tampering: hosts-file entries that block security or update sites or send popular sites elsewhere, proxies, unknown DNS servers typed in by hand, and browser extensions forced on by policy.
- **Drivers with known security holes or known to be malicious**, from the LOLDrivers list (shipped with Syscura, no download).
- Privacy tweaks that block Windows telemetry are recognised and not flagged.

### Drives, backups and a weekly summary
- **Drive health over time:** one reading a day (health, wear, temperature), shown on the Hardware page. Syscura warns when wear rises fast, a drive nears the end of its rated life, runs hot, or its health drops.
- **Repeating backups:** the Backup page can repeat a backup every few days while Syscura runs in the tray. Each run copies only what changed and never deletes anything. If the drive is unplugged, Syscura waits and reminds you.
- **Weekly summary:** one notification a week with what Syscura found and fixed. The Overview shows the week and the security score too.

### Any PC
- **Native Windows on ARM (ARM64)** builds and installer for Snapdragon laptops, tested on Windows 11 ARM. An x64 copy on an ARM PC updates to the native build.
- Graphics load now shows for every GPU maker (Intel, AMD, NVIDIA, Qualcomm), from Windows' own counters.

### More
- Settings: turn the daily update check and the weekly summary on or off.
- Command line: `syscura-cli security`, `syscura-cli summary` and `syscura-cli disks`.

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
