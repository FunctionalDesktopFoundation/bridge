#include "sharedsettings.h"
#include <QAndroidJniObject>
#include <QtAndroid>

static QAndroidJniObject getPrefs() {
    QAndroidJniObject context = QtAndroid::androidContext();
    return context.callObjectMethod(
        "getSharedPreferences",
        "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
        QAndroidJniObject::fromString("fdf_shared_settings").object(),
        0);
}

SharedSettings::SharedSettings(QObject *parent) : QObject(parent) {
    load();
}

void SharedSettings::load() {
    QAndroidJniObject prefs = getPrefs();
    m_darkMode = prefs.callBooleanMethod(
        "getBoolean", "(Ljava/lang/String;Z)Z",
        QAndroidJniObject::fromString("darkMode").object(), false);

    int accent = prefs.callIntMethod(
        "getInt", "(Ljava/lang/String;I)I",
        QAndroidJniObject::fromString("accentColor").object(), 0x4A90D9);
    m_accentColor = QColor::fromRgb(accent);

    m_themeName = prefs.callObjectMethod(
        "getString", "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        QAndroidJniObject::fromString("themeName").object(),
        QAndroidJniObject::fromString("system").object()).toString();
}

void SharedSettings::save() {
    QAndroidJniObject prefs = getPrefs();
    QAndroidJniObject editor = prefs.callObjectMethod(
        "edit", "()Landroid/content/SharedPreferences$Editor;");

    editor.callObjectMethod("putBoolean",
        "(Ljava/lang/String;Z)Landroid/content/SharedPreferences$Editor;",
        QAndroidJniObject::fromString("darkMode").object(), m_darkMode);
    editor.callObjectMethod("putInt",
        "(Ljava/lang/String;I)Landroid/content/SharedPreferences$Editor;",
        QAndroidJniObject::fromString("accentColor").object(),
        (jint)m_accentColor.rgb());
    editor.callObjectMethod("putString",
        "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
        QAndroidJniObject::fromString("themeName").object(),
        QAndroidJniObject::fromString(m_themeName).object());
    editor.callObjectMethod("apply", "()V");
}

void SharedSettings::setDarkMode(bool isDark) {
    if (m_darkMode == isDark) return;
    m_darkMode = isDark;
    save();
    emit darkModeChanged();
}

void SharedSettings::setAccentColor(const QColor &color) {
    if (m_accentColor == color) return;
    m_accentColor = color;
    save();
    emit accentColorChanged();
}

void SharedSettings::setThemeName(const QString &name) {
    if (m_themeName == name) return;
    m_themeName = name;
    save();
    emit themeNameChanged();
}
