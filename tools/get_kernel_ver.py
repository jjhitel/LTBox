import re
import sys
from pathlib import Path

def get_kernel_version(file_path):
    """
    Extracts and prints the Linux kernel version string from a file.
    """
    kernel_file = Path(file_path)
    if not kernel_file.exists():
        print(f"Error: Kernel file not found at '{file_path}'", file=sys.stderr)
        sys.exit(1)

    try:
        content = kernel_file.read_bytes()
        # Search for 'Linux version X.Y.Z-...'
        match = re.search(b'Linux version (\\S+)', content)
        if match:
            version = match.group(1).decode('utf-8', errors='ignore')
            print(version)
        else:
            print("Error: Kernel version string not found in the file.", file=sys.stderr)
            sys.exit(1)
    except Exception as e:
        print(f"An error occurred while reading the file: {e}", file=sys.stderr)
        sys.exit(1)

def main():
    """
    Main function to handle command-line arguments.
    """
    if len(sys.argv) > 1:
        kernel_file_path = sys.argv[1]
    else:
        # Default to 'kernel' if no argument is provided
        kernel_file_path = 'kernel'
    
    get_kernel_version(kernel_file_path)

if __name__ == "__main__":
    main()