import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import spinwave

// Waiting surface for the native sign-in window. Rust starts an ephemeral
// `spinwave --sign-in` window; the user finishes the Google flow in that window, and
// this dialog polls until Rust has the cookie header and the live gate probe
// completes. Cancel kills the child and closes its window.
M3Dialog {
    id: browserLogin

    property string errorText: ""
    property string progressText: qsTr("A secure browser window opened. Finish signing in there.")
    property bool completing: false
    property bool completed: false

    signal requestCookieFallback()

    icon: browserLogin.errorText.length > 0 ? "error" : "open_in_browser"
    headline: browserLogin.errorText.length > 0
        ? qsTr("Browser sign-in did not finish")
        : browserLogin.completing ? qsTr("Completing sign-in…") : qsTr("Continue in the browser window")
    width: 440
    closePolicy: Popup.CloseOnEscape

    contentItem: ColumnLayout {
        spacing: 14

        BusyIndicator {
            Layout.alignment: Qt.AlignHCenter
            running: !browserLogin.completed && browserLogin.errorText.length === 0
            visible: running
            Layout.preferredWidth: 42
            Layout.preferredHeight: 42
        }



        Label {
            Layout.fillWidth: true
            text: browserLogin.errorText.length > 0 ? browserLogin.errorText : browserLogin.progressText
            color: browserLogin.errorText.length > 0 ? Theme.tertiary : Theme.onSurfaceVariant
            font.pixelSize: 13
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
        }

        Label {
            Layout.fillWidth: true
            visible: browserLogin.errorText.length === 0 && !browserLogin.completing
            text: qsTr("Your password is entered in the browser window, never in this app.")
            font.pixelSize: 11
            color: Theme.onSurfaceVariant
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.topMargin: 8
            spacing: 8
            Item { Layout.fillWidth: true }
            PillButton {
                implicitHeight: 40
                fontSize: 14
                text: qsTr("Use a cookie instead")
                outlined: true
                fg: Theme.primary
                visible: browserLogin.errorText.length > 0
                onClicked: {
                    browserLogin.requestCookieFallback();
                    browserLogin.close();
                }
            }
            PillButton {
                implicitHeight: 40
                fontSize: 14
                text: qsTr("Cancel")
                bg: "transparent"
                fg: Theme.primary
                onClicked: browserLogin.close()
            }
        }
    }

    Timer {
        id: pollTimer
        interval: 500
        repeat: true
        onTriggered: {
            var result = Backend.pollBrowserSignin();
            if (result === "ok") {
                browserLogin.completed = true;
                pollTimer.stop();
                browserLogin.close();
            } else if (result === "finishing") {
                pollTimer.stop();
                browserLogin.completing = true;
                browserLogin.progressText = qsTr("Signing you in and checking Premium…");
                finishTimer.start();
            } else if (result !== "pending") {
                pollTimer.stop();
                browserLogin.errorText = result;
            }
        }
    }

    // One frame to paint "Completing sign-in…" before the blocking probe.
    Timer {
        id: finishTimer
        interval: 100
        onTriggered: {
            var result = Backend.finishBrowserSignin();
            if (result === "ok") {
                browserLogin.completed = true;
                browserLogin.close();
            } else {
                browserLogin.errorText = result;
                browserLogin.completing = false;
                browserLogin.progressText = "";
            }
        }
    }

    onOpened: {
        errorText = "";
        completing = false;
        completed = false;
        progressText = qsTr("A secure browser window opened. Finish signing in there.");
        var result = Backend.startBrowserSignin();
        if (result === "started") {
            pollTimer.start();
        } else {
            errorText = result;
        }
    }

    onClosed: {
        pollTimer.stop();
        finishTimer.stop();
        if (!completed) {
            Backend.cancelBrowserSignin();
        }
    }
}
