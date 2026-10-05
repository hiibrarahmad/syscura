# Changelog

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
