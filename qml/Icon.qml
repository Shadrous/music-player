import QtQuick
import spinwave

// Material Symbols Rounded glyph by ligature name (`play_arrow`, `favorite`).
// The font is installed per-user (see README); `filled` drives the FILL axis.
Text {
    property string name
    property bool filled: false
    property int size: 24

    text: name
    color: Theme.onSurfaceVariant
    font.family: "Material Symbols Rounded"
    font.pixelSize: size
    font.variableAxes: ({ "FILL": filled ? 1 : 0, "wght": filled ? 500 : 400, "opsz": 24 })
    // Distance-field text fills overlapping contours even-odd, so filled
    // glyphs showed holes; FreeType rasterizes them correctly.
    renderType: Text.NativeRendering
    horizontalAlignment: Text.AlignHCenter
    verticalAlignment: Text.AlignVCenter
    Accessible.ignored: true
}
