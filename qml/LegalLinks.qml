import QtQuick
import music_player

// "YouTube Terms of Service · Privacy Policy" footer (URLs from Rust).
Text {
    textFormat: Text.RichText
    wrapMode: Text.WordWrap
    color: Theme.onSurfaceVariant
    font.family: "Roboto"
    font.pixelSize: 11
    text: qsTr('<a href="%1" style="color:%3;text-decoration:none">YouTube Terms of Service</a> · <a href="%2" style="color:%3;text-decoration:none">Privacy Policy</a>')
        .arg(Backend.termsUrl()).arg(Backend.privacyUrl()).arg(Theme.onSurfaceVariant)
    onLinkActivated: (link) => Qt.openUrlExternally(link)
    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.NoButton
        cursorShape: parent.hoveredLink.length > 0 ? Qt.PointingHandCursor : Qt.ArrowCursor
    }
}
