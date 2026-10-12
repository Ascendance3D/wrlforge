#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Generate the WRL Forge --smoke-texture fixture set and its plan.json.

Usage: python3 smoke-texture-plan.py <dir>

<dir> must be an existing, EMPTY directory whose realpath lies strictly under
the system temp directory. Output is deterministic (no randomness, no
timestamps; gzip uses mtime=0).
"""

import gzip
import io
import json
import os
import sys
import tempfile

from PIL import Image

COLORS = {
    "red": (230, 30, 30),
    "green": (30, 200, 60),
    "blue": (40, 60, 230),
    "yellow": (240, 220, 30),
    "magenta": (220, 40, 200),
    "cyan": (30, 210, 220),
    "orange": (245, 140, 20),
}

SIZE = 64

TEMPLATE = (
    "#VRML V2.0 utf8\n"
    "# TEXTURE-LOCAL-1 fixture {fid}\n"
    'Viewpoint {{ position 0 0 10 description "front" }}\n'
    "Shape {{\n"
    "  appearance Appearance {{ texture ImageTexture {{ url [ {urls} ] }} }}\n"
    "  geometry Box {{ size 30 30 0.1 }}\n"
    "}}\n"
)

T09_TEXT = "\n".join([
    "#VRML V2.0 utf8",
    "# TEXTURE-LOCAL-1 fixture t09-def-use",
    'Viewpoint { position 0 0 10 description "front" }',
    "Transform { translation -4 0 0 children Shape {",
    '  appearance Appearance { texture DEF Atlas ImageTexture { url [ "green.png" ] } }',
    "  geometry Box { size 7.5 15 0.1 } } }",
    "Transform { translation 4 0 0 children Shape {",
    "  appearance Appearance { texture USE Atlas }",
    "  geometry Box { size 7.5 15 0.1 } } }",
]) + "\n"


def vrml_string(value):
    """Return value as a VRML string literal with \\ and " escaped."""
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


def vrml_text(fid, urls):
    url_list = " ".join(vrml_string(u) for u in urls)
    return TEMPLATE.format(fid=fid, urls=url_list)


def image_bytes(color_name, fmt):
    img = Image.new("RGB", (SIZE, SIZE), COLORS[color_name])
    buf = io.BytesIO()
    if fmt == "jpg":
        img.save(buf, "JPEG", quality=95, subsampling=0)
    elif fmt == "png":
        img.save(buf, "PNG")
    elif fmt == "gif":
        img.convert("P", palette=Image.ADAPTIVE).save(buf, "GIF")
    else:
        raise ValueError(fmt)
    return buf.getvalue()


def emit(root, rel, data):
    """Write data (bytes) to root/rel, where rel uses forward slashes."""
    path = os.path.join(root, *rel.split("/"))
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as handle:
        handle.write(data)


def emit_text(root, rel, text):
    emit(root, rel, text.encode("utf-8"))


def sample(color_name, x=0.5, y=0.5):
    return {"x": x, "y": y, "color": list(COLORS[color_name]) if color_name else None}


def untextured():
    return [{"x": 0.5, "y": 0.5, "color": None}]


def build(root):
    fixtures = []

    def add(fid, rel_file, gz, samples, warnings, label):
        fixtures.append({
            "id": fid,
            "file": rel_file,
            "gzip": gz,
            "samples": samples,
            "warnings": warnings,
            "label": label,
        })

    # Outside target that traversal and symlink cases must NOT reach.
    emit(root, "red-outside.jpg", image_bytes("red", "jpg"))

    # t01: plain VRML, JPG in the same folder.
    emit_text(root, "t01/doc.wrl", vrml_text("t01-jpg-same-folder", ["red.jpg"]))
    emit(root, "t01/red.jpg", image_bytes("red", "jpg"))
    add("t01-jpg-same-folder", "t01/doc.wrl", False,
        [sample("red")], 0, "Plain VRML with a JPG in the same folder.")

    # t02: gzip-compressed bytes in a .wrl file.
    t02_text = vrml_text("t02-gzip-jpg", ["red.jpg"])
    emit(root, "t02/doc.wrl", gzip.compress(t02_text.encode("utf-8"), mtime=0))
    emit(root, "t02/red.jpg", image_bytes("red", "jpg"))
    add("t02-gzip-jpg", "t02/doc.wrl", True,
        [sample("red")], 0, "Gzip-compressed VRML bytes in a .wrl file with a JPG.")

    # t03: PNG.
    emit_text(root, "t03/doc.wrl", vrml_text("t03-png", ["green.png"]))
    emit(root, "t03/green.png", image_bytes("green", "png"))
    add("t03-png", "t03/doc.wrl", False,
        [sample("green")], 0, "PNG texture from the same folder.")

    # t04: GIF in palette mode.
    emit_text(root, "t04/doc.wrl", vrml_text("t04-gif", ["blue.gif"]))
    emit(root, "t04/blue.gif", image_bytes("blue", "gif"))
    add("t04-gif", "t04/doc.wrl", False,
        [sample("blue")], 0, "GIF texture converted to palette mode.")

    # t05: nested child folder.
    emit_text(root, "t05/doc.wrl", vrml_text("t05-child-folder", ["tex/sub/yellow.png"]))
    emit(root, "t05/tex/sub/yellow.png", image_bytes("yellow", "png"))
    add("t05-child-folder", "t05/doc.wrl", False,
        [sample("yellow")], 0, "Texture in a nested child folder.")

    # t06: spaces in document and texture paths.
    emit_text(root, "t06 with spaces/my world.wrl",
              vrml_text("t06-spaces", ["my texture.jpg"]))
    emit(root, "t06 with spaces/my texture.jpg", image_bytes("magenta", "jpg"))
    add("t06-spaces", "t06 with spaces/my world.wrl", False,
        [sample("magenta")], 0, "Document and texture paths contain spaces.")

    # t07: first candidate wins when both exist.
    emit_text(root, "t07/doc.wrl", vrml_text("t07-fallback-first-wins", ["cyan.png", "red.jpg"]))
    emit(root, "t07/cyan.png", image_bytes("cyan", "png"))
    emit(root, "t07/red.jpg", image_bytes("red", "jpg"))
    add("t07-fallback-first-wins", "t07/doc.wrl", False,
        [sample("cyan")], 0, "First of two candidate URLs wins when both exist.")

    # t08: first candidate missing, fallback used.
    emit_text(root, "t08/doc.wrl", vrml_text("t08-missing-first", ["missing.jpg", "orange.png"]))
    emit(root, "t08/orange.png", image_bytes("orange", "png"))
    add("t08-missing-first", "t08/doc.wrl", False,
        [sample("orange")], 0, "First candidate URL is missing, so the fallback is used.")

    # t09: DEF/USE shares one texture across two shapes.
    emit_text(root, "t09/doc.wrl", T09_TEXT)
    emit(root, "t09/green.png", image_bytes("green", "png"))
    add("t09-def-use", "t09/doc.wrl", False,
        [sample("green", 0.3, 0.5), sample("green", 0.7, 0.5)], 0,
        "DEF/USE shares one texture across two shapes.")

    # t10: referenced image does not exist.
    emit_text(root, "t10/doc.wrl", vrml_text("t10-missing", ["nothere.jpg"]))
    add("t10-missing", "t10/doc.wrl", False,
        untextured(), 1, "Referenced image does not exist.")

    # t11: referenced file is not a valid image.
    emit_text(root, "t11/doc.wrl", vrml_text("t11-invalid-bytes", ["garbage.jpg"]))
    emit(root, "t11/garbage.jpg", b"this is not an image\n" * 20)
    add("t11-invalid-bytes", "t11/doc.wrl", False,
        untextured(), 1, "Referenced file is not a valid image.")

    # t12: traversal URLs must not reach the outside target.
    emit_text(root, "t12/doc.wrl", vrml_text("t12-traversal", [
        "../red-outside.jpg",
        "..%2fred-outside.jpg",
        "%2e%2e/red-outside.jpg",
        "sub/../../red-outside.jpg",
    ]))
    os.makedirs(os.path.join(root, "t12", "sub"), exist_ok=True)
    add("t12-traversal", "t12/doc.wrl", False,
        untextured(), 1, "Traversal URLs must not reach the outside target.")

    # t13: symlinks must not escape the document folder.
    emit_text(root, "t13/doc.wrl", vrml_text("t13-symlink-escape",
                                             ["link.jpg", "linkdir/red-outside.jpg"]))
    os.symlink("../red-outside.jpg", os.path.join(root, "t13", "link.jpg"))
    os.symlink("..", os.path.join(root, "t13", "linkdir"))
    add("t13-symlink-escape", "t13/doc.wrl", False,
        untextured(), 1, "Symlinks must not escape the document folder.")

    # t14: remote and absolute URLs are never loaded.
    abs_outside = os.path.realpath(os.path.join(root, "red-outside.jpg"))
    emit_text(root, "t14/doc.wrl", vrml_text("t14-remote-and-absolute", [
        "http://example.invalid/red.jpg",
        "https://example.invalid/red.png",
        "file://" + abs_outside,
        abs_outside,
    ]))
    add("t14-remote-and-absolute", "t14/doc.wrl", False,
        untextured(), 1, "Remote and absolute URLs are never loaded.")

    # t15: document switch while a texture request is active.
    emit_text(root, "t15/doc.wrl", vrml_text("t15-slow-switch", ["green.png"]))
    emit(root, "t15/green.png", image_bytes("green", "png"))
    add("t15-slow-switch", "t15/doc.wrl", False,
        [sample("green")], 0, "document switch while a texture request is active")

    # t16: save, close, reopen, reload.
    emit_text(root, "t16/doc.wrl", vrml_text("t16-save-reopen", ["red.jpg"]))
    emit(root, "t16/red.jpg", image_bytes("red", "jpg"))
    add("t16-save-reopen", "t16/doc.wrl", False,
        [sample("red")], 0, "save, close, reopen, reload")

    return fixtures


def check_dir(arg):
    if not os.path.isdir(arg):
        sys.stderr.write(f"error: {arg!r} is not an existing directory\n")
        sys.exit(2)
    real = os.path.realpath(arg)
    tmp = os.path.realpath(tempfile.gettempdir())
    try:
        under_tmp = real != tmp and os.path.commonpath([real, tmp]) == tmp
    except ValueError:
        under_tmp = False
    if not under_tmp:
        sys.stderr.write(f"error: {real} is not under the system temp dir {tmp}\n")
        sys.exit(2)
    if os.listdir(real):
        sys.stderr.write(f"error: {real} is not empty\n")
        sys.exit(2)
    return real


def main(argv):
    if len(argv) != 2:
        sys.stderr.write("usage: python3 smoke-texture-plan.py <dir>\n")
        return 2
    root = check_dir(argv[1])

    fixtures = build(root)
    plan = {"fixtures": fixtures, "outside": "red-outside.jpg"}
    plan_path = os.path.join(root, "plan.json")
    with open(plan_path, "wb") as handle:
        handle.write((json.dumps(plan, indent=2, sort_keys=False) + "\n").encode("utf-8"))

    for fixture in fixtures:
        print(f"{fixture['id']}: {fixture['label']}")
    print(f"plan ready: {plan_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
