import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import spinwave

// M3 menu row: leading icon, label, trailing chevron for submenus, rounded
// hover/press state layer. Hidden rows collapse to zero height.
MenuItem {
    id: item
    // Submenu rows take the submenu's `glyph`.
    property string glyph: subMenu && subMenu.glyph ? subMenu.glyph : ""
    property bool danger: false
    readonly property color fg: danger ? Theme.tertiary : Theme.onSurface

    height: visible ? implicitHeight : 0
    implicitHeight: 44
    leftPadding: 12
    rightPadding: 12
    opacity: enabled ? 1 : 0.38

    arrow: null
    indicator: null
    contentItem: RowLayout {
        spacing: 12
        Icon {
            visible: item.glyph.length > 0
            name: item.glyph
            size: 20
            color: item.danger ? Theme.tertiary : Theme.onSurfaceVariant
        }
        Text {
            Layout.fillWidth: true
            text: item.text
            color: item.fg
            font.family: "Roboto"
            font.pixelSize: 14
            font.weight: Font.Medium
            elide: Text.ElideRight
        }
        Icon {
            visible: item.subMenu !== null
            name: "chevron_right"
            size: 20
        }
    }
    background: Rectangle {
        implicitWidth: 200
        radius: 12
        color: item.fg
        opacity: !item.enabled ? 0 : item.pressed ? 0.12 : item.highlighted || item.hovered ? 0.08 : 0
    }
}
