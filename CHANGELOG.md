# Changelog

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
