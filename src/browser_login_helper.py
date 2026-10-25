#!/usr/bin/env python3
"""Ephemeral WebKitGTK sign-in window for Spinwave.

The app starts this helper for the "Continue with browser" action. It opens
Google's YouTube Music sign-in flow in a throwaway WebKitGTK data store, polls
the cookie manager for the session proof cookie, and prints the resulting
``Cookie`` request-header value on stdout exactly once. The process exits 0
after printing; closing the window first exits 2. Nothing is written to disk.

Stderr is for short, static diagnostics only — never cookie values.
"""

import os
import sys

try:
    import gi

    gi.require_version("Gtk", "3.0")
    gi.require_version("WebKit2", "4.1")
    from gi.repository import GLib, Gtk, WebKit2
except Exception as exc:  # noqa: BLE001 - startup dependency check
    print(f"WebKitGTK sign-in unavailable: {exc}", file=sys.stderr)
    sys.exit(3)

# Google's own sign-in hop, told to return to YouTube Music. The same URL
# Sonora's sign-in window opens.
SIGN_IN_URL = (
    "https://accounts.google.com/ServiceLogin?ltmpl=music&service=youtube"
    "&passive=true&continue=https%3A%2F%2Fwww.youtube.com%2Fsignin"
    "%3Faction_handle_signin%3Dtrue%26next%3Dhttps%253A%252F%252F"
    "music.youtube.com%252F"
)
LANDING = "https://music.youtube.com/"
PROOF = "__Secure-3PAPISID"
# Short diagnostics only; never cookie values.
DEBUG = os.environ.get("SPINWAVE_LOGIN_DEBUG") == "1"
# WebKitGTK's default agent claims Safari; Google answers it with a browser
# warning. Firefox is a browser Google accepts, and Sonora uses the same one.
USER_AGENT = (
    "Mozilla/5.0 (X11; Linux x86_64; rv:134.0) "
    "Gecko/20100101 Firefox/134.0"
)


class LoginWindow:
    def __init__(self):
        self.done = False
        self.poll_id = None
        self.reported_cookies = False

        manager = WebKit2.WebsiteDataManager.new_ephemeral()
        self.context = WebKit2.WebContext.new_with_website_data_manager(manager)
        self.cookies = self.context.get_cookie_manager()
        self.cookies.set_accept_policy(WebKit2.CookieAcceptPolicy.ALWAYS)

        self.window = Gtk.Window(title="Sign in to YouTube Music")
        self.window.set_default_size(1000, 760)
        self.window.set_position(Gtk.WindowPosition.CENTER)
        self.window.connect("destroy", self.on_destroy)

        self.view = WebKit2.WebView.new_with_context(self.context)
        settings = self.view.get_settings()
        settings.set_user_agent(USER_AGENT)
        settings.set_javascript_can_open_windows_automatically(True)
        self.view.connect("load-failed", self.on_load_failed)
        if DEBUG:
            self.view.connect("load-changed", self.on_load_changed)

        self.window.add(self.view)
        self.window.show_all()
        self.window.present()
        self.view.load_uri(SIGN_IN_URL)
        self.poll_id = GLib.timeout_add(600, self.poll)

    def poll(self):
        self.cookies.get_cookies(LANDING, None, self.on_cookies, None)
        return True

    def on_cookies(self, manager, result, _data):
        if self.done:
            return
        try:
            cookies = manager.get_cookies_finish(result)
        except GLib.Error:
            return

        pairs = []
        proof = False
        for cookie in cookies:
            name = cookie.get_name()
            value = cookie.get_value()
            if not name or not value:
                continue
            if name == PROOF:
                proof = True
            pairs.append(f"{name}={value}")

        if not proof:
            if DEBUG and pairs and not self.reported_cookies:
                self.reported_cookies = True
                names = sorted({p.split("=", 1)[0] for p in pairs})
                print(f"cookies seen: {', '.join(names)}", file=sys.stderr)
            return

        self.done = True
        if self.poll_id is not None:
            GLib.source_remove(self.poll_id)
            self.poll_id = None
        sys.stdout.write("; ".join(pairs) + "\n")
        sys.stdout.flush()
        GLib.idle_add(self.quit)

    def quit(self):
        self.window.destroy()
        Gtk.main_quit()
        return False

    def on_destroy(self, *_args):
        if not self.done:
            Gtk.main_quit()

    def on_load_failed(self, _view, _event, _uri, error):
        print(f"sign-in page failed to load: {error.message}", file=sys.stderr)

    def on_load_changed(self, view, event):
        print(f"load {event.value_nick}: {view.get_uri()}", file=sys.stderr)


def main():
    window = LoginWindow()
    Gtk.main()
    return 0 if window.done else 2


if __name__ == "__main__":
    sys.exit(main())
