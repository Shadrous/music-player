#!/usr/bin/env bash
# Download the UI fonts (open licenses) plus their license texts into $1.
# Packages load them from there; dev checkouts use installed fonts instead.
set -euo pipefail
out="$1"
mkdir -p "$out"
gf=https://raw.githubusercontent.com/google/fonts/main/ofl
icons=https://raw.githubusercontent.com/google/material-design-icons/master
get() { curl -fsSL --retry 3 -o "$out/$1" "$2"; }
get Roboto.ttf "$gf/roboto/Roboto%5Bwdth%2Cwght%5D.ttf"
get Roboto-OFL.txt "$gf/roboto/OFL.txt"
get GoogleSansFlex.ttf "$gf/googlesansflex/GoogleSansFlex%5BROND%2Copsz%2Cslnt%2Cwdth%2Cwght%5D.ttf"
get GoogleSansFlex-OFL.txt "$gf/googlesansflex/OFL.txt"
get MaterialSymbolsRounded.ttf "$icons/variablefont/MaterialSymbolsRounded%5BFILL%2CGRAD%2Copsz%2Cwght%5D.ttf"
get MaterialSymbols-LICENSE.txt "$icons/LICENSE"
