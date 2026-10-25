import QtQuick
import QtWebEngine
import spinwave

// `spinwave --sign-in`: Google's YouTube Music sign-in in an off-the-record
// web view. The Rust side (sign_in.rs) watches its cookies and exits.
Window {
    width: 1000
    height: 760
    visible: true
    color: Theme.surface
    title: qsTr("Sign in to YouTube Music")

    WebEngineView {
        id: view
        anchors.fill: parent
        Component.onCompleted: {
            // Google turns away agents that announce an embedded engine.
            profile.httpUserAgent = profile.httpUserAgent.replace(/ QtWebEngine\/\S+/, "");
            if (!profile.offTheRecord)
                console.warn("sign-in: profile is not off-the-record");
            url = "https://accounts.google.com/ServiceLogin?ltmpl=music&service=youtube"
                + "&passive=true&continue=https%3A%2F%2Fwww.youtube.com%2Fsignin"
                + "%3Faction_handle_signin%3Dtrue%26next%3Dhttps%253A%252F%252Fmusic.youtube.com%252F";
        }
        onNewWindowRequested: (request) => view.url = request.requestedUrl
    }
}
