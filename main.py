# main.py
import os
import sys
import shutil
import subprocess
import argparse
import requests
import zipfile
from pathlib import Path

BASE_DIR = Path(__file__).parent.resolve()
TOOLS_DIR = BASE_DIR / "tools"
PYTHON_EXE = BASE_DIR / "python3" / "python.exe"
KEY_DIR = BASE_DIR / "key"
AVBTOOL_PY = TOOLS_DIR / "avbtool.py"
EDIT_VNDRBOOT_PY = TOOLS_DIR / "edit_vndrboot.py"
PARSE_INFO_PY = TOOLS_DIR / "parse_info.py"
GET_KERNEL_VER_PY = TOOLS_DIR / "get_kernel_ver.py"

TARGET_KERNEL_VERSION = "6.1.112"
ANYKERNEL_URL = "https://github.com/WildKernels/GKI_KernelSU_SUSFS/releases/download/v1.5.9-r36/WKSU-13861-android14-6.1.112-2024-11-AnyKernel3.zip"
ANYKERNEL_ZIP = "AnyKernel3.zip"

def run_command(command, shell=False, check=True):
    try:
        env = os.environ.copy()
        env['PATH'] = str(TOOLS_DIR) + os.pathsep + env['PATH']

        process = subprocess.run(
            command, shell=shell, check=check, capture_output=True, text=True, encoding='utf-8', errors='ignore', env=env
        )
        if process.stdout:
            print(process.stdout.strip())
        if process.stderr:
            print(process.stderr.strip(), file=sys.stderr)
        return process
    except subprocess.CalledProcessError as e:
        print(f"Error executing command: {' '.join(map(str, command))}", file=sys.stderr)
        print(f"Return code: {e.returncode}", file=sys.stderr)
        if e.stdout:
            print(f"Stdout:\n{e.stdout.strip()}", file=sys.stderr)
        if e.stderr:
            print(f"Stderr:\n{e.stderr.strip()}", file=sys.stderr)
        raise
    except FileNotFoundError:
        print(f"Error: Command not found - {command[0]}", file=sys.stderr)
        raise

def check_dependencies():
    print("--- Checking for required files ---")
    dependencies = {
        "Python Environment": PYTHON_EXE,
        "RSA4096 Key": KEY_DIR / "testkey_rsa4096.pem",
        "RSA2048 Key": KEY_DIR / "testkey_rsa2048.pem",
        "avbtool": AVBTOOL_PY
    }
    for name, path in dependencies.items():
        if not path.exists():
            print(f"[!] Error: Dependency '{name}' is missing.")
            print("Please run 'install.bat' first to download all required files.")
            sys.exit(1)
    print("[+] All dependencies are present.")
    print()

def patch_boot_with_root():
    print("--- Starting boot.img patching process ---")
    magiskboot_exe = TOOLS_DIR / "magiskboot.exe"

    if not magiskboot_exe.exists():
        print("[!] 'magiskboot.exe' not found. Attempting to download...")
        try:
            url = 'https://github.com/CYRUS-STUDIO/MagiskBootWindows/raw/refs/heads/main/magiskboot.exe'
            response = requests.get(url, stream=True)
            response.raise_for_status()
            with open(magiskboot_exe, 'wb') as f:
                for chunk in response.iter_content(chunk_size=8192):
                    f.write(chunk)
            print("[+] Download successful.")
        except requests.RequestException as e:
            print(f"[!] Download failed: {e}. Aborting.")
            sys.exit(1)

    boot_img = BASE_DIR / "boot.img"
    if not boot_img.exists():
        print("[!] 'boot.img' not found! Aborting.")
        sys.exit(1)

    shutil.copy(boot_img, BASE_DIR / "boot.bak.img")
    print("--- Backing up original boot.img ---")

    work_dir = BASE_DIR / "patch_work"
    if work_dir.exists():
        shutil.rmtree(work_dir)
    work_dir.mkdir()

    original_cwd = os.getcwd()
    os.chdir(work_dir)

    try:
        shutil.copy(boot_img, work_dir)

        print("\n[1/6] Unpacking boot image...")
        run_command([str(magiskboot_exe), "unpack", "boot.img"])
        if not (work_dir / "kernel").exists():
            print("[!] Failed to unpack boot.img. The image might be invalid.")
            sys.exit(1)
        print("[+] Unpack successful.")

        print("\n[2/6] Verifying kernel version...")
        result = run_command([str(PYTHON_EXE), str(GET_KERNEL_VER_PY), "kernel"])
        extracted_version = result.stdout.strip()
        print(f"[+] Found version string: {extracted_version}")
        if not extracted_version.startswith(TARGET_KERNEL_VERSION):
            print(f"[!] ERROR: Kernel version is NOT {TARGET_KERNEL_VERSION}.")
            print("Script will now abort to prevent damage.")
            sys.exit(1)
        print("[+] Kernel version matches.")

        print("\n[3/6] Downloading GKI Kernel...")
        try:
            response = requests.get(ANYKERNEL_URL, stream=True)
            response.raise_for_status()
            with open(ANYKERNEL_ZIP, 'wb') as f:
                 for chunk in response.iter_content(chunk_size=8192):
                    f.write(chunk)
            print("[+] Download complete.")
        except requests.RequestException as e:
            print(f"[!] Failed to download the kernel zip file: {e}.")
            sys.exit(1)

        print("\n[4/6] Extracting new kernel image...")
        extracted_kernel_dir = work_dir / "extracted_kernel"
        with zipfile.ZipFile(ANYKERNEL_ZIP, 'r') as zip_ref:
            zip_ref.extractall(extracted_kernel_dir)
        if not (extracted_kernel_dir / "Image").exists():
            print("[!] 'Image' file not found in the downloaded zip.")
            sys.exit(1)
        print("[+] Extraction successful.")

        print("\n[5/6] Replacing original kernel with the new one...")
        shutil.move(str(extracted_kernel_dir / "Image"), "kernel")
        print("[+] Kernel replaced.")

        print("\n[6/6] Repacking boot image...")
        run_command([str(magiskboot_exe), "repack", "boot.img"])
        if not (work_dir / "new-boot.img").exists():
            print("[!] Failed to repack the boot image.")
            sys.exit(1)
        shutil.move("new-boot.img", BASE_DIR / "boot.root.img")
        print("[+] Repack successful.")

    finally:
        os.chdir(original_cwd)
        if work_dir.exists():
            shutil.rmtree(work_dir)
        if boot_img.exists():
            boot_img.unlink()
        print("\n--- Cleaning up ---")

    print("\n" + "=" * 61)
    print("  SUCCESS!")
    print(f"  Patched image has been saved as: {BASE_DIR / 'boot.root.img'}")
    print("=" * 61)
    print("\n--- Handing over to convert process ---\n")


def convert_images(with_root=False):
    if with_root:
        patch_boot_with_root()

    check_dependencies()

    print("[*] Cleaning up old folders...")
    if (BASE_DIR / "output").exists():
        shutil.rmtree(BASE_DIR / "output")
    print()

    print("--- Backing up original images ---")
    vendor_boot_img = BASE_DIR / "vendor_boot.img"
    vbmeta_img = BASE_DIR / "vbmeta.img"

    if not vendor_boot_img.exists():
        print("[!] 'vendor_boot.img' not found! Aborting.")
        sys.exit(1)
    if not vbmeta_img.exists():
        print("[!] 'vbmeta.img' not found! Aborting.")
        sys.exit(1)

    vendor_boot_bak = BASE_DIR / "vendor_boot.bak.img"
    vbmeta_bak = BASE_DIR / "vbmeta.bak.img"
    shutil.move(vendor_boot_img, vendor_boot_bak)
    shutil.copy(vbmeta_img, vbmeta_bak)
    print("[+] Backup complete.\n")

    print("--- Starting PRC/ROW Conversion ---")
    run_command([str(PYTHON_EXE), str(EDIT_VNDRBOOT_PY), str(vendor_boot_bak)])

    vendor_boot_prc = BASE_DIR / "vendor_boot_prc.img"
    print("\n[*] Verifying conversion result...")
    if not vendor_boot_prc.exists():
        print("[!] 'vendor_boot_prc.img' was not created. No changes made.")
        sys.exit(1)
    print("[+] Conversion to PRC successful.\n")

    print("--- Extracting Image Information ---")
    info_proc = run_command([
        str(PYTHON_EXE), str(PARSE_INFO_PY), str(vendor_boot_bak), str(AVBTOOL_PY), str(vbmeta_bak)
    ])

    img_info = dict(line.split('=', 1) for line in info_proc.stdout.strip().split('\n') if '=' in line)

    prop_val_clean = img_info['PROP_VAL'][1:-1]
    
    print("\n--- Adding Hash Footer to vendor_boot ---")
    prop_val_file = BASE_DIR / "prop_val.tmp"
    with open(prop_val_file, "w", encoding='utf-8') as f:
        f.write(prop_val_clean)

    add_hash_footer_cmd = [
        str(PYTHON_EXE), str(AVBTOOL_PY), "add_hash_footer",
        "--image", str(vendor_boot_prc),
        "--partition_size", img_info['IMG_SIZE'],
        "--partition_name", "vendor_boot",
        "--rollback_index", "0",
        "--salt", img_info['SALT'],
        "--prop_from_file", f"{img_info['PROP_KEY']}:{prop_val_file}"
    ]
    run_command(add_hash_footer_cmd)
    
    if prop_val_file.exists():
        prop_val_file.unlink()
    print()

    key_file = ""
    public_key = img_info.get('PUBLIC_KEY')
    if public_key == "2597c218aae470a130f61162feaae70afd97f011":
        key_file = KEY_DIR / "testkey_rsa4096.pem"
    elif public_key == "cdbb77177f731920bbe0a0f94f84d9038ae0617d":
        key_file = KEY_DIR / "testkey_rsa2048.pem"

    if with_root:
        print("--- Processing boot image ---")
        boot_bak_img = BASE_DIR / "boot.bak.img"
        boot_info_proc = run_command([str(PYTHON_EXE), str(AVBTOOL_PY), "info_image", "--image", str(boot_bak_img)])

        boot_info = {}
        boot_props_args = []
        for line in boot_info_proc.stdout.strip().split('\n'):
            line = line.strip()
            if line.startswith("Image size:"):
                boot_info['size'] = line.split()[-2]
            elif line.startswith("Partition Name:"):
                boot_info['name'] = line.split()[-1]
            elif line.startswith("Salt:"):
                boot_info['salt'] = line.split()[-1]
            elif line.startswith("Rollback Index:"):
                boot_info['rollback'] = line.split()[-1]
            elif line.startswith("Prop:"):
                parts = line.split('->')
                key = parts[0].split(':')[-1].strip()
                val = parts[1].strip()[1:-1]
                boot_props_args.extend(["--prop", f"{key}:{val}"])
        
        print("\n[*] Adding new hash footer to 'boot.root.img'...")
        boot_root_img = BASE_DIR / "boot.root.img"
        add_footer_cmd = [
            str(PYTHON_EXE), str(AVBTOOL_PY), "add_hash_footer",
            "--image", str(boot_root_img),
            "--key", str(key_file),
            "--algorithm", img_info['ALGORITHM'],
            "--partition_size", boot_info['size'],
            "--partition_name", boot_info['name'],
            "--rollback_index", boot_info['rollback'],
            "--salt", boot_info['salt']
        ] + boot_props_args
        run_command(add_footer_cmd)
        print()

    print("--- Re-signing vbmeta.img ---")
    print("[*] Verifying vbmeta key...")
    if not key_file:
        print(f"[!] Public key '{public_key}' did not match known keys. Aborting.")
        sys.exit(1)
    print(f"[+] Matched {key_file.name}.")

    print("\n[*] Re-signing 'vbmeta.img' using backup descriptors...")
    resign_cmd = [
        str(PYTHON_EXE), str(AVBTOOL_PY), "make_vbmeta_image",
        "--output", str(vbmeta_img),
        "--key", str(key_file),
        "--algorithm", img_info['ALGORITHM'],
        "--padding_size", "8192",
        "--include_descriptors_from_image", str(vbmeta_bak),
        "--include_descriptors_from_image", str(vendor_boot_prc)
    ]
    run_command(resign_cmd)
    print()

    print("--- Finalizing ---")
    print("[*] Renaming final images...")
    final_vendor_boot = BASE_DIR / "vendor_boot.img"
    shutil.move(vendor_boot_prc, final_vendor_boot)

    final_images = [final_vendor_boot, vbmeta_img]
    if with_root:
        final_boot = BASE_DIR / "boot.img"
        shutil.move(BASE_DIR / "boot.root.img", final_boot)
        final_images.append(final_boot)

    print("\n[*] Moving final images to 'output' folder...")
    output_dir = BASE_DIR / "output"
    output_dir.mkdir(exist_ok=True)
    for img in final_images:
        shutil.move(img, output_dir / img.name)

    print("\n[*] Moving backup files to 'backup' folder...")
    backup_dir = BASE_DIR / "backup"
    backup_dir.mkdir(exist_ok=True)
    for bak_file in BASE_DIR.glob("*.bak.img"):
        shutil.move(bak_file, backup_dir / bak_file.name)
    print()

    print("=" * 61)
    print("  SUCCESS!")
    print("  Final images have been saved to the 'output' folder.")
    print("=" * 61)

def show_image_info(files):
    print("\n" + "=" * 42)
    print("  Sorted and Processing Images...")
    print("=" * 42 + "\n")

    sorted_files = sorted(files)

    for f in sorted_files:
        file_path = Path(f).resolve()
        if not file_path.exists():
            print(f"File not found: {file_path}")
            continue

        print(f"Processing file: {file_path.name}")
        print("---------------------------------")
        try:
            run_command([
                str(PYTHON_EXE), str(AVBTOOL_PY), "info_image", "--image", str(file_path)
            ])
        except subprocess.CalledProcessError:
             print(f"Failed to get info from {file_path.name}")
        print("---------------------------------\n")

def main():
    parser = argparse.ArgumentParser(description="Android vendor_boot Patcher and vbmeta Resigner.")
    subparsers = parser.add_subparsers(dest="command", required=True)

    parser_convert = subparsers.add_parser("convert", help="Convert vendor_boot and re-sign vbmeta.")
    parser_convert.add_argument("--with-root", action="store_true", help="Patch boot.img with KernelSU before converting.")

    parser_info = subparsers.add_parser("info", help="Display information about image files.")
    parser_info.add_argument("files", nargs='+', help="Image file(s) to inspect.")

    args = parser.parse_args()

    try:
        if args.command == "convert":
            convert_images(args.with_root)
        elif args.command == "info":
            show_image_info(args.files)
    except (subprocess.CalledProcessError, FileNotFoundError, SystemExit) as e:
        if isinstance(e, SystemExit):
            pass
        else:
            print(f"\nAn unexpected error occurred: {e}", file=sys.stderr)
    finally:
        print()
        os.system("pause")


if __name__ == "__main__":
    main()