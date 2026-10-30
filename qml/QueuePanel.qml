import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import spinwave

// Queue side panel: the current song, then the upcoming lane. Rows dragged
// in from any list land in the gap they're dropped on; each upcoming row's
// handle drags it to a new place. Layout only: the queue lives in Rust.
Rectangle {
    id: panel
    signal close()

    radius: 28
    color: Theme.surfaceContainerLow

    TrackModel { id: upcoming }
    property var current: null
    property int upcomingCount: 0
    // Gap a drag would drop into (-1 = no drag over the panel).
    property int dropGap: -1

    function refresh() {
        if (!visible) return
        var cur = Backend.currentRowJson()
        panel.current = cur.length > 0 ? JSON.parse(cur) : null
        upcoming.loadJson(Backend.queueRowsJson())
        panel.upcomingCount = upcoming.rowCount()
    }
    // Upcoming gap under panel point `y`: before the row it's in, or after
    // it when in the row's lower half.
    function gapAt(y) {
        var cy = list.mapFromItem(panel, 0, y).y + list.contentY
        if (cy < 0) return 0
        var i = list.indexAt(16, cy)
        if (i < 0) return panel.upcomingCount
        var item = list.itemAtIndex(i)
        return cy - item.y > item.height / 2 ? i + 1 : i
    }
    function gapY(gap) {
        var item = list.itemAtIndex(Math.min(gap, panel.upcomingCount - 1))
        if (!item) return list.y
        var y = gap < panel.upcomingCount ? item.y : item.y + item.height
        return list.y + y - list.contentY
    }

    Component.onCompleted: refresh()
    onVisibleChanged: refresh()
    Connections {
        target: Backend
        function onQueueChanged() { panel.refresh() }
        function onVideoIdChanged() { panel.refresh() }
        function onQueueCountChanged() { panel.refresh() }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 8
        spacing: 4

        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 12
            Layout.topMargin: 6
            Text {
                Layout.fillWidth: true
                text: qsTr("Up next")
                color: Theme.onSurface
                font.family: "Google Sans Flex"
                font.pixelSize: 22
                font.weight: Font.DemiBold
            }
            IconButton {
                icon: "close"
                label: qsTr("Close queue")
                onClicked: panel.close()
            }
        }
        Toggle {
            Layout.leftMargin: 12
            Layout.bottomMargin: 6
            text: qsTr("Autoplay similar songs")
            checked: Backend.autoplayOn
            onToggled: Backend.toggleAutoplay()
        }

        Loader {
            Layout.fillWidth: true
            active: panel.current !== null
            visible: active
            sourceComponent: TrackRow {
                title: panel.current.title
                artist: panel.current.artist
                album: panel.current.album
                video_id: panel.current.video_id
                duration: panel.current.duration
                thumbnail_url: panel.current.thumbnail_url
                artist_id: panel.current.artist_id
                album_id: panel.current.album_id
                set_video_id: panel.current.set_video_id
                index: 0
                draggable: false
            }
        }

        ListView {
            id: list
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: upcoming
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar {}

            delegate: Item {
                id: slot
                required property int index
                required property string title
                required property string artist
                required property string album
                required property string video_id
                required property string duration
                required property string thumbnail_url
                required property string artist_id
                required property string album_id
                required property string set_video_id
                width: list.width
                height: 56

                // Handle: drags this row to a new place in the lane.
                Item {
                    id: handle
                    width: 32
                    height: parent.height
                    Icon {
                        anchors.centerIn: parent
                        name: "drag_handle"
                        size: 20
                        color: Theme.onSurfaceVariant
                    }
                    MouseArea {
                        property real pressY: 0
                        anchors.fill: parent
                        // Keep the list from scrolling instead.
                        preventStealing: true
                        cursorShape: Qt.SizeAllCursor
                        onPressed: (mouse) => {
                            pressY = mouse.y
                            slotChip.grabToImage(function(result) { handle.Drag.imageSource = result.url })
                        }
                        onPositionChanged: (mouse) => {
                            if (!handle.Drag.active && Math.abs(mouse.y - pressY) > 6) handle.Drag.active = true
                        }
                    }
                    Drag.dragType: Drag.Automatic
                    Drag.supportedActions: Qt.MoveAction
                    Drag.mimeData: ({ "text/x-spinwave-queue-index": String(slot.index) })
                    Drag.hotSpot.x: 16
                    Drag.hotSpot.y: 16
                    Drag.onDragFinished: handle.Drag.active = false
                    Accessible.role: Accessible.Button
                    Accessible.name: qsTr("Reorder %1").arg(slot.title)
                }
                Rectangle {
                    id: slotChip
                    x: -10000
                    width: Math.min(240, chipLabel.implicitWidth + 32)
                    height: 36
                    radius: 18
                    color: Theme.secondaryContainer
                    Text {
                        id: chipLabel
                        anchors.centerIn: parent
                        width: parent.width - 32
                        text: slot.title
                        color: Theme.onSecondaryContainer
                        font.family: "Google Sans Flex"
                        font.pixelSize: 14
                        elide: Text.ElideRight
                    }
                }
                TrackRow {
                    anchors.left: handle.right
                    anchors.right: parent.right
                    title: slot.title
                    artist: slot.artist
                    album: slot.album
                    video_id: slot.video_id
                    duration: slot.duration
                    thumbnail_url: slot.thumbnail_url
                    artist_id: slot.artist_id
                    album_id: slot.album_id
                    set_video_id: slot.set_video_id
                    index: slot.index
                    draggable: false
                    playHandler: function() { Backend.playQueueRow(slot.video_id) }
                }
            }

            footer: Text {
                width: list.width
                topPadding: 12
                leftPadding: 16
                // The panel lists the next 100 (UP_NEXT_ROWS in Rust).
                visible: Backend.queueCount - 1 > panel.upcomingCount && panel.upcomingCount > 0
                text: qsTr("+ %1 more in the queue").arg(Backend.queueCount - 1 - panel.upcomingCount)
                color: Theme.onSurfaceVariant
                font.family: "Roboto"
                font.pixelSize: 14
            }
        }
    }

    Text {
        anchors.centerIn: parent
        width: parent.width - 48
        visible: panel.upcomingCount === 0
        horizontalAlignment: Text.AlignHCenter
        wrapMode: Text.Wrap
        text: qsTr("Nothing up next. Drag songs here or use Add to queue.")
        color: Theme.onSurfaceVariant
        font.family: "Roboto"
        font.pixelSize: 14
    }

    // Where a dragged row would land.
    Rectangle {
        visible: panel.dropGap >= 0
        x: 16
        width: parent.width - 32
        height: 3
        radius: 1.5
        y: panel.gapY(panel.dropGap) - 1
        color: Theme.primary
    }

    DropArea {
        anchors.fill: parent
        keys: ["text/x-spinwave-row", "text/x-spinwave-queue-index"]
        onPositionChanged: (drag) => {
            panel.dropGap = panel.gapAt(drag.y)
            // Near an edge: scroll so far rows can be reached.
            var y = list.mapFromItem(panel, 0, drag.y).y
            if (y < 40) list.contentY = Math.max(0, list.contentY - 16)
            else if (y > list.height - 40) list.contentY = Math.min(Math.max(0, list.contentHeight - list.height), list.contentY + 16)
        }
        onExited: panel.dropGap = -1
        onDropped: (drop) => {
            var gap = panel.gapAt(drop.y)
            panel.dropGap = -1
            if (drop.formats.indexOf("text/x-spinwave-queue-index") >= 0) {
                Backend.queueMove(parseInt(drop.getDataAsString("text/x-spinwave-queue-index")), gap)
            } else {
                Backend.queueAddRow(drop.getDataAsString("text/x-spinwave-row"), gap)
            }
            drop.accept()
        }
    }
}
