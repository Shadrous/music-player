import QtQuick
import QtQuick.Effects
import spinwave

// Async cover art clipped to a rounded rectangle. `ready` is true once the
// image loaded, so callers can show their fallback underneath until then.
Item {
    id: round
    property alias source: img.source
    // Decode size (small row icons decode small, not at download size).
    property alias sourceSize: img.sourceSize
    property real radius: 16
    readonly property bool ready: img.status === Image.Ready && String(img.source).length > 0

    Image {
        id: img
        anchors.fill: parent
        asynchronous: true
        cache: true
        fillMode: Image.PreserveAspectCrop
        // The software scenegraph can't run MultiEffect: square corners there.
        visible: GraphicsInfo.api === GraphicsInfo.Software
    }
    Rectangle {
        id: mask
        anchors.fill: parent
        radius: round.radius
        layer.enabled: true
        visible: false
    }
    MultiEffect {
        anchors.fill: parent
        source: img
        maskEnabled: true
        maskSource: mask
        // Soft mask edge = antialiased corners.
        maskThresholdMin: 0.5
        maskSpreadAtMin: 1.0
        visible: round.ready && !img.visible
    }
}
