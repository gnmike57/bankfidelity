import re
from pathlib import Path

LAUNCHERS_DIR = Path(r"C:\bankfidelity\bankfidelity\launchers")
DESKTOP_DIRS = [
    Path(r"C:\Users\zbook\Desktop"),
    Path(r"C:\Users\zbook\OneDrive\Desktop"),
]

ansi_re = re.compile(r"!ESC!\[[0-9;]*[a-zA-Z]|\[[0-9;]*[a-zA-Z]|\x1b\[[0-9;]*[a-zA-Z]")

def sanitize_bat_content(text: str) -> str:
    # 1. Remove ANSI escape tokens
    text = ansi_re.sub("", text)
    # 2. Remove ESC definition boilerplate if present
    lines = text.splitlines()
    clean_lines = []
    for line in lines:
        if "set \"ESC=" in line or "echo prompt $E" in line:
            continue
        # Replace lone unquoted & in echo commands with 'and' or '&'
        if line.strip().startswith("echo") and "&" in line and not ("&" in line and ("&&" in line or '"' in line or "^&" in line)):
            # Replace raw & with 'and' or escape it with ^&
            # Keep % and other batch vars
            parts = line.split("echo", 1)
            prefix = parts[0] + "echo"
            body = parts[1].replace("&", "and")
            line = prefix + body
        clean_lines.append(line)
    return "\n".join(clean_lines) + "\n"

def main():
    print(f"Sanitizing launchers in {LAUNCHERS_DIR}...")
    for bat_file in LAUNCHERS_DIR.glob("*.bat"):
        raw = bat_file.read_text(encoding="utf-8", errors="ignore")
        sanitized = sanitize_bat_content(raw)
        bat_file.write_text(sanitized, encoding="utf-8")
        print(f"  [OK] Sanitized {bat_file.name}")
        
        # Copy to desktops
        for d in DESKTOP_DIRS:
            if d.exists():
                try:
                    dest = d / bat_file.name
                    dest.write_text(sanitized, encoding="utf-8")
                except Exception as e:
                    pass

if __name__ == "__main__":
    main()
