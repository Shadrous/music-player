import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Shapes
import spinwave

// Docked player: now-playing + like | shuffle, prev, play, next, repeat |
// volume + up-next toggle, then a wavy progress lane that seeks on click
// or drag (the seek lands on release).
// Layout only: all state lives in the Backend singleton.
Rectangle {
    id: card
    property bool queueOpen: false
    property bool lyricsOn: false
    // Lyrics need the art pane; the shell hides it in narrow windows.
    property bool lyricsAvailable: true
    signal toggleQueue()
    signal toggleLyrics()
    signal requestUpgrade()

    implicitHeight: column.implicitHeight + 26
    radius: 28
    color: Theme.surfaceContainerHigh

    readonly property bool loading: Backend.playbackState === "loading"
    readonly property real progress: Backend.duration > 0 ? Math.min(1, Math.max(0, Backend.position / Backend.duration)) : 0
    // While the lane is held, show where the drag would seek to.
    readonly property real shownProgress: seek.pressed ? seek.fraction : progress
    readonly property real shownPosition: seek.pressed ? seek.fraction * Backend.duration : Backend.position

    function fmt(s) {
        s = Math.max(0, Math.round(s));
        var m = Math.floor(s / 60);
        var r = s % 60;
        return m + ":" + (r < 10 ? "0" : "") + r;
    }

    // The single position tick: mirrors the decoder and auto-advances at
    // the end of a track (Rust side).
    Timer {
        interval: 1000
        running: true
        repeat: true
        onTriggered: Backend.tickPosition()
    }

    ColumnLayout {
        id: column
        anchors.fill: parent
        anchors.leftMargin: 16
        anchors.rightMargin: 16
        anchors.topMargin: 12
        anchors.bottomMargin: 14
        spacing: 6

        // Three equal columns keep the transport centred.
        Item {
            id: controls
            Layout.fillWidth: true
            implicitHeight: 56
            // Room on either side of the centred transport.
            readonly property real sideWidth: (width - transport.width) / 2 - 12
            // Narrow bars keep prev / play / next only in the middle.
            readonly property bool essentialsOnly: width < 640

            RowLayout {
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
                width: controls.sideWidth
                spacing: 12

                Rectangle {
                    Layout.preferredWidth: 56
                    Layout.preferredHeight: 56
                    radius: 16
                    clip: true
                    color: Theme.primaryContainer
                    Blob {
                        x: -14; y: 14; width: 60; height: 60
                        visible: !thumb.ready
                        fillColor: Theme.secondaryContainer
                        spinning: Backend.isPlaying && !Backend.lowPerformance
                    }
                    Rectangle {
                        x: 36; y: 4; width: 28; height: 28; radius: 14
                        visible: !thumb.ready
                        color: Theme.primary
                    }
                    RoundImage {
                        id: thumb
                        anchors.fill: parent
                        radius: 16
                        source: Backend.coverSource
                    }
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    // Title opens the song's album (a single's own page),
                    // the artist line the artist page.
                    Text {
                        Layout.fillWidth: true
                        text: Backend.title.length > 0 ? Backend.title : qsTr("Nothing playing")
                        color: Theme.onSurface
                        font.family: "Google Sans Flex"
                        font.pixelSize: 16
                        font.weight: Font.DemiBold
                        font.underline: titleLink.containsMouse
                        elide: Text.ElideRight
                        MouseArea {
                            id: titleLink
                            width: Math.min(parent.implicitWidth, parent.width)
                            height: parent.height
                            enabled: Backend.title.length > 0 && !card.loading
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: Backend.openCurrentAlbum()
                        }
                    }
                    Text {
                        Layout.fillWidth: true
                        text: card.loading ? qsTr("Loading…")
                            : Backend.playbackState === "error" ? Backend.errorMessage
                            : Backend.playbackState === "upgrade" ? qsTr("Premium required — tap for details")
                            : Backend.artist
                        color: Backend.playbackState === "error" ? Theme.tertiary : Theme.onSurfaceVariant
                        font.family: "Roboto"
                        font.pixelSize: 13
                        font.underline: artistLink.containsMouse && !upgradeLink.enabled
                        elide: Text.ElideRight
                        MouseArea {
                            id: upgradeLink
                            anchors.fill: parent
                            enabled: Backend.playbackState === "upgrade"
                            cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
                            onClicked: card.requestUpgrade()
                        }
                        MouseArea {
                            id: artistLink
                            width: Math.min(parent.implicitWidth, parent.width)
                            height: parent.height
                            enabled: !upgradeLink.enabled && !card.loading
                                && Backend.playbackState !== "error" && Backend.artist.length > 0
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: Backend.openCurrentArtist()
                        }
                    }
                }
                IconButton {
                    icon: "favorite"
                    label: Backend.liked ? qsTr("Unlike song") : qsTr("Like song")
                    iconSize: 22
                    toggle: true
                    checked: Backend.liked
                    fg: Backend.liked ? Theme.primary : Theme.onSurfaceVariant
                    enabled: Backend.isLoggedIn && Backend.videoId.length > 0
                    onClicked: Backend.toggleHeart()
                }
            }

            RowLayout {
                id: transport
                anchors.centerIn: parent
                spacing: 6
                IconButton {
                    visible: !controls.essentialsOnly
                    icon: "shuffle"
                    label: qsTr("Shuffle")
                    iconSize: 22
                    toggle: true
                    checked: Backend.shuffle
                    onClicked: Backend.toggleShuffle()
                }
                IconButton {
                    implicitWidth: 48; implicitHeight: 48
                    icon: "skip_previous"
                    label: qsTr("Previous")
                    filled: true
                    bg: Theme.surfaceContainerHighest
                    fg: Theme.onSurface
                    enabled: !card.loading
                    onClicked: Backend.prev()
                }
                IconButton {
                    // Morphs from a 56px circle to a 76px rounded pill while playing.
                    implicitWidth: Backend.isPlaying ? 76 : 56
                    implicitHeight: 56
                    restRadius: Backend.isPlaying ? 18 : 28
                    icon: Backend.isPlaying ? "pause" : "play_arrow"
                    label: Backend.isPlaying ? qsTr("Pause") : qsTr("Play")
                    filled: true
                    iconSize: 30
                    bg: Theme.primary
                    fg: Theme.onPrimary
                    enabled: !card.loading && Backend.videoId.length > 0
                    Behavior on implicitWidth { enabled: !Backend.lowPerformance; NumberAnimation { duration: 300; easing.type: Easing.OutBack } }
                    onClicked: Backend.togglePlay()
                }
                IconButton {
                    implicitWidth: 48; implicitHeight: 48
                    icon: "skip_next"
                    label: qsTr("Next")
                    filled: true
                    bg: Theme.surfaceContainerHighest
                    fg: Theme.onSurface
                    enabled: !card.loading
                    onClicked: Backend.next()
                }
                IconButton {
                    visible: !controls.essentialsOnly
                    icon: Backend.loopMode === "one" ? "repeat_one" : "repeat"
                    label: Backend.loopMode === "off" ? qsTr("Repeat off")
                        : Backend.loopMode === "all" ? qsTr("Repeat all") : qsTr("Repeat one")
                    iconSize: 22
                    toggle: true
                    checked: Backend.loopMode !== "off"
                    onClicked: Backend.cycleLoop()
                }
                IconButton {
                    visible: card.lyricsAvailable && !controls.essentialsOnly
                    icon: "mic_external_on"
                    label: card.lyricsOn ? qsTr("Hide lyrics") : qsTr("Show lyrics")
                    iconSize: 22
                    toggle: true
                    checked: card.lyricsOn
                    onClicked: card.toggleLyrics()
                }
            }

            RowLayout {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                spacing: 4
                IconButton {
                    // Level to restore on unmute (0.8 if muted from the start).
                    property real unmutedVolume: 0.8
                    icon: Backend.volume === 0 ? "volume_off" : Backend.volume < 0.5 ? "volume_down" : "volume_up"
                    label: Backend.volume === 0 ? qsTr("Unmute") : qsTr("Mute")
                    iconSize: 22
                    onClicked: {
                        if (Backend.volume > 0) {
                            unmutedVolume = Backend.volume;
                            Backend.setVolume(0);
                        } else {
                            Backend.setVolume(unmutedVolume);
                        }
                    }
                }
                Slider {
                    id: volume
                    // Mute button + slider + Up next (40 + 96 + 40 + spacing);
                    // narrower than that, drop the slider instead of
                    // sliding under the transport.
                    visible: controls.sideWidth >= 184
                    Layout.preferredWidth: 96
                    // Custom parts below set no implicit size, so give the hit area one.
                    Layout.preferredHeight: 32
                    from: 0
                    to: 1
                    value: Backend.volume
                    onMoved: Backend.setVolume(value)
                    Accessible.name: qsTr("Volume")
                    background: Rectangle {
                        x: volume.leftPadding
                        y: volume.topPadding + volume.availableHeight / 2 - height / 2
                        width: volume.availableWidth
                        height: 4
                        radius: 2
                        color: Theme.secondaryContainer
                        Rectangle {
                            width: volume.visualPosition * parent.width
                            height: parent.height
                            radius: 2
                            color: Theme.primary
                        }
                    }
                    // Cursor only; presses still reach the slider.
                    MouseArea {
                        anchors.fill: parent
                        acceptedButtons: Qt.NoButton
                        cursorShape: Qt.PointingHandCursor
                    }
                    handle: Rectangle {
                        x: volume.leftPadding + volume.visualPosition * (volume.availableWidth - width)
                        y: volume.topPadding + volume.availableHeight / 2 - height / 2
                        width: 4; height: 20; radius: 2
                        color: Theme.primary
                    }
                }
                IconButton {
                    icon: "queue_music"
                    label: qsTr("Up next")
                    iconSize: 22
                    toggle: true
                    checked: card.queueOpen
                    onClicked: card.toggleQueue()
                }
            }
        }

        // Progress lane: position · wave + handle + remaining track · -remaining.
        RowLayout {
            Layout.fillWidth: true
            spacing: 12
            Text {
                Layout.preferredWidth: 36
                text: card.fmt(card.shownPosition)
                color: Theme.onSurfaceVariant
                font.family: "Roboto"
                font.pixelSize: 12
                font.features: { "tnum": 1 }
            }
            Item {
                id: lane
                Layout.fillWidth: true
                implicitHeight: 24
                readonly property real playedWidth: Math.max(0, (width - 8) * card.shownProgress)

                Item {
                    width: lane.playedWidth
                    height: 20
                    anchors.verticalCenter: parent.verticalCenter
                    clip: true
                    Shape {
                        id: wave
                        width: 2400; height: 20
                        // CurveRenderer ignores the parent clip; geometry + MSAA doesn't.
                        layer.enabled: true
                        layer.samples: 4
                        ShapePath {
                            strokeColor: Theme.primary
                            strokeWidth: 4
                            capStyle: ShapePath.RoundCap
                            fillColor: "transparent"
                            PathSvg {
                                // Design: `q6 -7 12 0` then `t12 0` repeats; flat while paused.
                                path: Backend.isPlaying
                                    ? "M0 10 q6 -7 12 0" + " t12 0".repeat(199)
                                    : "M2 10 H2398"
                            }
                        }
                        NumberAnimation on x {
                            from: -24; to: 0
                            duration: 1100
                            loops: Animation.Infinite
                            running: Backend.isPlaying && !Backend.lowPerformance
                        }
                    }
                }
                Rectangle {
                    x: lane.playedWidth + 2
                    anchors.verticalCenter: parent.verticalCenter
                    width: 4; height: 24; radius: 2
                    color: Theme.primary
                }
                Rectangle {
                    x: lane.playedWidth + 10
                    anchors.verticalCenter: parent.verticalCenter
                    width: Math.max(0, lane.width - x)
                    height: 4; radius: 2
                    color: Theme.secondaryContainer
                }
                MouseArea {
                    id: seek
                    property real fraction: 0
                    anchors.fill: parent
                    enabled: !card.loading && Backend.duration > 0
                    preventStealing: true
                    cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
                    onPressed: (mouse) => fraction = Math.min(1, Math.max(0, mouse.x / width))
                    onPositionChanged: (mouse) => fraction = Math.min(1, Math.max(0, mouse.x / width))
                    onReleased: Backend.seekTo(Math.round(fraction * Backend.duration))
                }
                Accessible.role: Accessible.Slider
                Accessible.name: qsTr("Seek, %1 of %2").arg(card.fmt(Backend.position)).arg(card.fmt(Backend.duration))
            }
            Text {
                Layout.preferredWidth: 40
                horizontalAlignment: Text.AlignRight
                text: "-" + card.fmt(Math.max(0, Backend.duration - card.shownPosition))
                color: Theme.onSurfaceVariant
                font.family: "Roboto"
                font.pixelSize: 12
                font.features: { "tnum": 1 }
            }
        }
    }
}
