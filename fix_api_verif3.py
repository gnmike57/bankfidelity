with open("src/app/api_verification.rs", "r") as f:
    lines = f.readlines()

new_lines = lines[:330] + lines[370:]

with open("src/app/api_verification.rs", "w") as f:
    f.writelines(new_lines)
