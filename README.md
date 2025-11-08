# LTBox

## ⚠️ Important: Disclaimer

**This project is for educational purposes ONLY.**

Modifying your device's firmware carries significant risks, including but not limited to, bricking your device, data loss, or voiding your warranty. The author **assumes no liability** and is not responsible for any **damage or consequence** that may occur to **your device or anyone else's device** from using these scripts.

**You are solely responsible for any consequences. Use at your own absolute risk.**

---

## 1. Core Vulnerability & Overview

This toolkit exploits a security vulnerability found in certain Lenovo Android tablets. These devices have firmware signed with publicly available **AOSP (Android Open Source Project) test keys**.

Because of this vulnerability, the device's bootloader trusts and boots any image signed with these common test keys, even if the bootloader is **locked**.

This toolkit is an all-in-one collection of scripts that leverages this flaw to perform advanced modifications on a device with a locked bootloader.

### Target Models

* Lenovo Legion Y700 (2nd, 3rd, 4th Gen)
* Lenovo Tab Plus AI (aka Yoga Pad Pro AI)
* Lenovo Xiaoxin Pad Pro GT

*...Other recent Lenovo devices (released in 2024 or later with Qualcomm chipsets) may also be vulnerable.*

## 2. Toolkit Purpose & Features

This toolkit provides an all-in-one solution for the following tasks **without unlocking the bootloader**:

1.  **Region Conversion (PRC → ROW):** Converts Chinese (PRC) firmware to Global (ROW) firmware by patching `vendor_boot.img` and rebuilding `vbmeta.img`.

2.  **Get Root Access:** Replaces the kernel in `boot.img` with [GKI_KernelSU_SUSFS](https://github.com/WildKernels/GKI_KernelSU_SUSFS) for root access.

3.  **Anti-Rollback Bypass:** Bypasses rollback protection, allowing you to flash older (downgrade) firmware versions by patching the rollback index in `boot.img` and `vbmeta_system.img`.

4.  **Change Region Code:** Patches `devinfo.img` and `persist.img` to change region code.

5.  **Firmware Flashing:** Uses `edl-ng` and `fh_loader` to dump partitions and flash modified firmware packages in Qualcomm Emergency Download (EDL) mode.

6.  **Automated Process:** Provides fully automated options to perform all the above steps in the correct order, with options for both data wipe and data preservation (no wipe).

## 3. How to Use

The toolkit is now centralized into a single menu-driven script.

1.  **Run the Script:** Double-click **`start.bat`**.

2.  **Install Dependencies (First Run):** The first time you run `start.bat`, it will automatically execute `ltbox\install.bat`. This will download and install all required dependencies (Python, `adb`, `edl-ng`, `avbtool`, `fetch`, etc.) into the `python3/` and `tools/` folders.

3.  **Select Task:** Choose an option from the menu.

    * Main Menu (1, 2, 3...): For common, fully automated tasks.
    * Advanced Menu (a): For manual, step-by-step operations.

4.  **Follow Prompts:** The scripts will prompt you when you need to place files (e.g., "Waiting for image folder...") or connect your device (e.g., "Waiting for ADB/EDL device...").

5.  **Get Results:** After a task finishes, modified images are saved in the corresponding `output*` folder (e.g., `output/`, `output_root/`).

6.  **Flash the Images:** The Main Menu options and the "Flash Firmware" option handle this automatically. You can also flash individual `output*` images manually using the Advanced menu options.

## 4. Script Descriptions

### 4.1 Main Menu

These are the primary, automated functions.

**`1. Install ROW firmware to PRC device (WIPE DATA)`**

The all-in-one automated task. It performs all steps (Convert, XML Modify, Dump, Patch, ARB Check, Flash) and **wipes all user data**.

**`2. Update ROW firmware on PRC device (NO WIPE)`**

Same as option 1, but modifies the XML scripts to **preserve user data** (skips `userdata` and `metadata` partitions).

**`3. Disable OTA`**

Connects to the device in ADB mode and disables the `com.lenovo.ota` package to prevent automatic system updates.

**`4. Root Device`**

Connects to the device, reboots to EDL, dumps the *current* `boot.img`, change its kernel, and flashes it back to the device. This is the all-in-one method to root your device.

**`5. Unroot Device`**

Connects to the device, reboots to EDL, and flashes a stock `boot.img` back to the device. It will look for `boot.img` in the `backup_boot/` folder or prompt you to place it there.


### 4.2 Advanced Menu

These are the individual steps, allowing for manual control.

**`1. Convert ROW to PRC in ROM`**

Converts `vendor_boot.img` and rebuilds `vbmeta.img`. (Input: `image/`, Output: `output/`).

**`2. Dump devinfo/persist from device`**

Connect device in EDL mode. Dumps `devinfo` and `persist` to the `backup/` folder.

**`3. Patch devinfo/persist to change region code`**

Patches "CNXX" in `devinfo.img`/`persist.img`. It will prompt you to select a new country code from a list. (Input: `backup_critical/`, Output: `output_dp/`).

**`4. Write devinfo/persist to device`**

Flashes the patched images from `output_dp/` to the device via EDL.

**`5. Detect Anti-Rollback from device`**

Dumps current device partitions (`input_current/`) and compares their rollback indices to the new ROM (`image/`).

**`6. Patch rollback indices in ROM`**

If a downgrade is detected (by Step 5), this patches the new ROM's images with the device's *current* (higher) index. (Input: `image/`, Output: `output_anti_rollback/`).

**`7. Write Anti-Anti-Rollback images to device`**

Flashes the ARB-patched images from `output_anti_rollback/` to the device via EDL.

**`8. Convert x files to xml (WIPE DATA)`**

Decrypts `.x` files from `image/` to `.xml` for a **full data wipe**. (Output: `output_xml/`).

**`9. Convert x files to xml & Modify (NO WIPE)`**

Decrypts `.x` files and modifies them to **skip user data partitions**. (Output: `output_xml/`).

**`10. Flash firmware to device`**

Manual full flash. This complex step first copies all `output*` folders (`output/`, `output_root/`, `output_anti_rollback/`, `output_xml/`) into `image/` (overwriting). It then flashes `image/` using `fh_loader` (XMLs), and *then* separately flashes patched images from `output_dp/` and `output_anti_rollback/`.

**`11. Clean workspace`**

Deletes all `output*`, `input*`, `image`, `work`, `working`, `working_boot` folders, downloaded tools, and temp files. Does **not** delete the `backup/` or `backup_boot/` folders.

## 5. Other Utilities

**`info_image.bat`**

It will run `avbtool.py` to get detailed info (partition name, rollback index, AVB properties) and save it to `image_info_[timestamp].txt`.