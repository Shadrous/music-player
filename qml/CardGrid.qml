import QtQuick
import QtQuick.Layouts
import spinwave

// Titled grid of compact cards (min 200px columns) over an AlbumModel or
// ArtistModel: art tile (or a blob fallback), title, meta line. Hidden when
// the model is empty.
ColumnLayout {
    id: grid
    property string title
    property var model
    property bool round: false
    signal picked(string title, string sub)

    visible: repeater.count > 0
    spacing: 10

    Text {
        Layout.leftMargin: 8
        text: grid.title
        color: Theme.onSurface
        font.family: "Google Sans Flex"
        font.pixelSize: 22
        font.weight: Font.DemiBold
    }

    GridLayout {
        Layout.fillWidth: true
        Layout.leftMargin: 4
        Layout.rightMargin: 4
        columns: Math.max(1, Math.floor((width + 8) / 208))
        columnSpacing: 8
        rowSpacing: 8

        Repeater {
            id: repeater
            model: grid.model
            delegate: Rectangle {
                id: card
                required property var model
                required property int index
                // Albums carry title/artist/year; artists name/subscribers.
                readonly property string name: model.title !== undefined ? model.title : model.name
                readonly property string sub: model.title !== undefined
                    ? [model.artist, model.year].filter(function(s) { return s.length > 0 }).join(" · ")
                    : model.subscribers
                readonly property var tones: [
                    [Theme.tertiaryContainer, Theme.onTertiaryContainer],
                    [Theme.secondaryContainer, Theme.onSecondaryContainer],
                    [Theme.primaryContainer, Theme.onPrimaryContainer]
                ]
                Layout.fillWidth: true
                Layout.preferredWidth: 200
                implicitHeight: 76
                radius: 24 // art (14) + 10 inset: concentric
                color: Theme.surfaceContainer
                Rectangle {
                    anchors.fill: parent
                    radius: parent.radius
                    color: Theme.onSurface
                    opacity: area.pressed ? 0.12 : area.containsMouse ? 0.08 : 0
                }
                RowLayout {
                    anchors.fill: parent
                    anchors.margins: 10
                    spacing: 12
                    Rectangle {
                        Layout.preferredWidth: 56
                        Layout.preferredHeight: 56
                        radius: grid.round ? 28 : 14
                        color: card.tones[card.index % 3][0]
                        Blob {
                            anchors.centerIn: parent
                            width: 40; height: 40
                            visible: !thumb.ready
                            lobes: [4, 6, 8][card.index % 3]
                            amp: [9, 5, 5][card.index % 3]
                            base: [40, 43, 44][card.index % 3]
                            fillColor: card.tones[card.index % 3][1]
                        }
                        RoundImage {
                            id: thumb
                            anchors.fill: parent
                            radius: parent.radius
                            source: card.model.thumbnail_url
                        }
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0
                        Text {
                            Layout.fillWidth: true
                            text: card.name
                            color: Theme.onSurface
                            font.family: "Google Sans Flex"
                            font.pixelSize: 16
                            font.weight: Font.Medium
                            elide: Text.ElideRight
                        }
                        Text {
                            Layout.fillWidth: true
                            visible: card.sub.length > 0
                            text: card.sub
                            color: Theme.onSurfaceVariant
                            font.family: "Roboto"
                            font.pixelSize: 12
                            elide: Text.ElideRight
                        }
                    }
                }
                MouseArea {
                    id: area
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: grid.picked(card.name, card.model.title !== undefined ? card.model.artist : "")
                }
                Accessible.role: Accessible.Button
                Accessible.name: card.name
                Accessible.onPressAction: area.clicked(null)
            }
        }
    }
}
