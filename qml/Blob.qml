import QtQuick
import QtQuick.Shapes
import spinwave

// Rounded M3 "cookie" shape (design `blob(n, amp, base)` on a 100x100 box),
// optionally spinning. Decorative only.
Item {
    id: blob
    property int lobes: 12
    property real amp: 3.2
    property real base: 45
    property color fillColor: Theme.secondaryContainer
    property bool spinning: false
    property bool reverse: false
    property int period: 24000

    function pathFor(n, a, b) {
        var d = "";
        for (var i = 0; i < 120; i++) {
            var t = 2 * Math.PI * i / 120;
            var r = b + a * Math.cos(n * t);
            d += (i ? "L" : "M") + (50 + r * Math.cos(t - Math.PI / 2)).toFixed(1)
                + " " + (50 + r * Math.sin(t - Math.PI / 2)).toFixed(1);
        }
        return d + "Z";
    }

    Shape {
        anchors.fill: parent
        preferredRendererType: Shape.CurveRenderer
        transform: Scale { xScale: blob.width / 100; yScale: blob.height / 100 }
        ShapePath {
            fillColor: blob.fillColor
            strokeWidth: -1
            PathSvg { path: blob.pathFor(blob.lobes, blob.amp, blob.base) }
        }
    }

    RotationAnimator on rotation {
        from: 0
        to: blob.reverse ? -360 : 360
        duration: blob.period
        loops: Animation.Infinite
        running: blob.spinning && blob.visible
    }
}
