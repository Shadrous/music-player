import QtQuick
import QtQuick.Layouts
import spinwave

// Song row (min 56px): number or equalizer, title + artist/album, liked
// heart, duration. The current track (matched by video id) sits on a
// secondary-container pill. Layout only: `playHandler` is the list owner's
// play action.
Item {
    id: row
    required property string title
    required property string artist
    required property string album
    required property string video_id
    required property string duration
    required property string thumbnail_url
    // Qt6 exposes the view's `index` to delegates with required props only
    // through a matching required property.
    required property int index
    property int rowIndex: index
    property var playHandler: null

    readonly property bool isCurrent: Backend.videoId.length > 0 && Backend.videoId === video_id
    readonly property string subLine: artist.length > 0 ? (album.length > 0 ? artist + " · " + album : artist) : album
    readonly property color subFg: isCurrent ? Theme.onSecondaryContainer : Theme.onSurfaceVariant

    implicitHeight: 56

    Rectangle {
        anchors.fill: parent
        // Card radius (28) minus its 8px inset.
        radius: 20
        color: row.isCurrent ? Theme.secondaryContainer : "transparent"
        Behavior on color { enabled: !Backend.lowPerformance; ColorAnimation { duration: 200 } }
        Rectangle {
            anchors.fill: parent
            radius: parent.radius
            color: row.isCurrent ? Theme.onSecondaryContainer : Theme.onSurface
            opacity: area.pressed ? 0.12 : area.containsMouse ? 0.08 : 0
            Behavior on opacity { enabled: !Backend.lowPerformance; NumberAnimation { duration: 200 } }
        }
    }

    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: if (row.playHandler) row.playHandler()
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 4
        anchors.rightMargin: 8
        spacing: 12

        Item {
            Layout.preferredWidth: 40
            Layout.fillHeight: true
            EqBars {
                anchors.centerIn: parent
                visible: row.isCurrent
                running: Backend.isPlaying && !Backend.lowPerformance
            }
            Text {
                anchors.centerIn: parent
                visible: !row.isCurrent
                text: row.rowIndex + 1 < 10 ? "0" + (row.rowIndex + 1) : "" + (row.rowIndex + 1)
                color: row.subFg
                font.family: "Google Sans Flex"
                font.pixelSize: 14
                font.features: { "tnum": 1 }
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignVCenter
            spacing: 0
            Text {
                Layout.fillWidth: true
                text: row.title
                color: row.isCurrent ? Theme.onSecondaryContainer : Theme.onSurface
                font.family: "Google Sans Flex"
                font.pixelSize: 16
                font.weight: row.isCurrent ? Font.Bold : Font.Medium
                elide: Text.ElideRight
            }
            Text {
                Layout.fillWidth: true
                visible: row.subLine.length > 0
                text: row.subLine
                color: row.subFg
                font.family: "Roboto"
                font.pixelSize: 13
                elide: Text.ElideRight
            }
        }

        Icon {
            visible: row.isCurrent && Backend.liked
            name: "favorite"
            filled: true
            size: 20
            color: Theme.onSecondaryContainer
            Accessible.ignored: false
            Accessible.name: qsTr("Liked")
        }

        Text {
            Layout.preferredWidth: 44
            text: row.duration
            color: row.subFg
            horizontalAlignment: Text.AlignRight
            font.family: "Roboto"
            font.pixelSize: 14
            font.features: { "tnum": 1 }
        }
    }

    Accessible.role: Accessible.Button
    Accessible.name: row.title + (row.artist.length > 0 ? ", " + row.artist : "")
    Accessible.onPressAction: if (row.playHandler) row.playHandler()
}
