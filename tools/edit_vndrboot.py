import sys
from pathlib import Path

def main():
    """
    Replaces byte patterns in a file.
    Usage: python edit_vndrboot.py <vendor_boot.img>
    """
    if len(sys.argv) != 2:
        print(f"Usage: python {sys.argv[0]} <vendor_boot.img>")
        sys.exit(1)

    input_file = Path(sys.argv[1])
    output_file = input_file.parent / "vendor_boot_prc.img"

    if not input_file.exists():
        print(f"Error: Input file not found at '{input_file}'")
        sys.exit(1)

    patterns_to_find = {
        b"\x2E\x52\x4F\x57": b"\x2E\x50\x52\x43",  # .ROW -> .PRC
        b"\x49\x52\x4F\x57": b"\x49\x50\x52\x43"   # IROW -> IPRC
    }

    try:
        content = input_file.read_bytes()
        modified_content = content
        found_count = 0

        for target, replacement in patterns_to_find.items():
            count = modified_content.count(target)
            if count > 0:
                print(f"Found '{target.hex().upper()}' pattern {count} time(s). Replacing...")
                modified_content = modified_content.replace(target, replacement)
                found_count += count

        if found_count > 0:
            output_file.write_bytes(modified_content)
            print(f"\nPatch successful! Total {found_count} instance(s) replaced.")
            print(f"Saved as '{output_file}'")
        else:
            print("No target patterns found. No changes were made.")

    except IOError as e:
        print(f"An error occurred during file operation: {e}")
        sys.exit(1)
    except Exception as e:
        print(f"An unexpected error occurred: {e}")
        sys.exit(1)

if __name__ == "__main__":
    main()