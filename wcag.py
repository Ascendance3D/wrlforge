import sys

def srgb_lin(v):
    v = v / 255.0
    if v <= 0.03928:
        return v / 12.92
    return ((v + 0.055) / 1.055) ** 2.4

def luminance(hex_str):
    hex_str = hex_str.lstrip('#')
    if len(hex_str) == 3:
        hex_str = hex_str[0]*2 + hex_str[1]*2 + hex_str[2]*2
    r = int(hex_str[0:2], 16)
    g = int(hex_str[2:4], 16)
    b = int(hex_str[4:6], 16)
    return 0.2126 * srgb_lin(r) + 0.7152 * srgb_lin(g) + 0.0722 * srgb_lin(b)

def contrast(hex1, hex2):
    l1 = luminance(hex1)
    l2 = luminance(hex2)
    bright = max(l1, l2)
    dark = min(l1, l2)
    return (bright + 0.05) / (dark + 0.05)

print("Tauri Contrast: primary button text (#ffffff) on bg (#005fcc)", f"{contrast('ffffff', '005fcc'):.2f}:1")
print("Tauri Contrast: focus (#00e0ff) on bg (#000000)", f"{contrast('00e0ff', '000000'):.2f}:1")
print("Tauri Contrast: text (#ffffff) on background (#000000)", f"{contrast('ffffff', '000000'):.2f}:1")

print("Electron HC: primary button text (#fff) on bg (#005fcc) - Assuming similar High Contrast theme in electron", f"{contrast('ffffff', '005fcc'):.2f}:1")
