import os
import re

def get_rs_files(directory):
    rs_files = set()
    for root, _, files in os.walk(directory):
        for file in files:
            if file.endswith('.rs'):
                rs_files.add(os.path.join(root, file))
    return rs_files

def extract_mods(filepath):
    mods = set()
    try:
        with open(filepath, 'r') as f:
            content = f.read()
            # Handle standard mod declarations
            matches = re.findall(r'^\s*pub\s+mod\s+([a-zA-Z0-9_]+)\s*;|^\s*mod\s+([a-zA-Z0-9_]+)\s*;', content, re.MULTILINE)
            for m in matches:
                mod_name = m[0] if m[0] else m[1]
                mods.add(mod_name)
    except Exception as e:
        print(f"Error reading {filepath}: {e}")
    return mods

def resolve_mod_path(parent_path, mod_name):
    # If parent is mod.rs, submodules are in the same directory
    # If parent is name.rs, submodules are in name/ directory
    parent_dir = os.path.dirname(parent_path)
    parent_name = os.path.basename(parent_path)
    
    if parent_name == 'mod.rs' or parent_name == 'main.rs' or parent_name == 'lib.rs':
        base_dir = parent_dir
    else:
        base_dir = os.path.join(parent_dir, parent_name[:-3])
        
    path1 = os.path.join(base_dir, f"{mod_name}.rs")
    path2 = os.path.join(base_dir, mod_name, "mod.rs")
    
    if os.path.exists(path1):
        return path1
    if os.path.exists(path2):
        return path2
    
    # Path3: sometimes it's mapped differently, let's just return what we expect
    return path1

def find_unused_files(src_dir):
    all_files = get_rs_files(src_dir)
    visited = set()
    
    def visit(filepath):
        if filepath in visited or not os.path.exists(filepath):
            return
        visited.add(filepath)
        mods = extract_mods(filepath)
        for mod in mods:
            mod_path = resolve_mod_path(filepath, mod)
            visit(mod_path)

    # Start points
    start_points = []
    if os.path.exists(os.path.join(src_dir, 'main.rs')):
        start_points.append(os.path.join(src_dir, 'main.rs'))
    if os.path.exists(os.path.join(src_dir, 'lib.rs')):
        start_points.append(os.path.join(src_dir, 'lib.rs'))
        
    # Also bin directories
    bin_dir = os.path.join(src_dir, 'bin')
    if os.path.exists(bin_dir):
        for f in os.listdir(bin_dir):
            if f.endswith('.rs'):
                start_points.append(os.path.join(bin_dir, f))
                
    for start in start_points:
        visit(start)
        
    # Exclude tests root if any, but we don't have a tests root in src
    # sometimes test_egui.rs or similar is included conditionally
    
    unused = all_files - visited
    return unused

if __name__ == "__main__":
    unused = find_unused_files("src")
    for f in sorted(unused):
        print(f)
