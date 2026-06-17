pragma Singleton
import QtQuick
import FDF.SharedSettings 1.0

QtObject {
    property bool darkMode: FDF_DARK_MODE
    property color accentColor: FDF_ACCENT_COLOR
    property string themeName: FDF_THEME_NAME

    onDarkModeChanged: {
        if (typeof sharedSettings !== "undefined")
            sharedSettings.darkMode = darkMode
    }

    function setDarkMode(v) {
        FDF_DARK_MODE = v
        if (typeof sharedSettings !== "undefined")
            sharedSettings.darkMode = v
    }

    function setAccentColor(c) {
        FDF_ACCENT_COLOR = c
        if (typeof sharedSettings !== "undefined")
            sharedSettings.accentColor = c
    }

    function setThemeName(t) {
        FDF_THEME_NAME = t
        if (typeof sharedSettings !== "undefined")
            sharedSettings.themeName = t
    }
}
