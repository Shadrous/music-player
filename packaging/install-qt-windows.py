"""Install Qt for MSVC from Qt's online repository into argv[1].

shortcut: replaces install-qt-action, whose aqt 3.3 predates Qt's per-compiler
repo folders (qt6_6112_msvc2022_64/); switch back once aqt supports them.
Usage: python install-qt-windows.py OUT_DIR [VERSION]   (needs 7z on PATH)
"""
import subprocess
import sys
import urllib.request
import xml.etree.ElementTree as ET

out = sys.argv[1]
version = sys.argv[2] if len(sys.argv) > 2 else "6.11.2"
v = version.replace(".", "")
repo = "https://download.qt.io/online/qtsdkrepository/windows_x86"
desktop = f"{repo}/desktop/qt6_{v}/qt6_{v}_msvc2022_64"
packages = [
    (desktop, f"qt.qt6.{v}.win64_msvc2022_64"),
]

indexes = {}
for base, name in packages:
    if base not in indexes:
        indexes[base] = ET.fromstring(urllib.request.urlopen(f"{base}/Updates.xml", timeout=60).read())
    package = next(p for p in indexes[base].findall("PackageUpdate") if p.findtext("Name") == name)
    pkg_version = package.findtext("Version")
    archives = [a.strip() for a in package.findtext("DownloadableArchives").split(",") if a.strip()]
    assert archives, f"{name} lists no archives"
    for archive in archives:
        print("Qt:", archive, flush=True)
        urllib.request.urlretrieve(f"{base}/{name}/{pkg_version}{archive}", archive)
        subprocess.run(["7z", "x", "-y", "-bso0", f"-o{out}", archive], check=True)
