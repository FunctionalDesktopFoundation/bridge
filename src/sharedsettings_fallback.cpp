#include "sharedsettings.h"
#include <QSettings>
#include <QApplication>
#include <QStyleHints>

static const char *kOrg = "FDF";
static const char *kApp = "SharedSettings";

SharedSettings::SharedSettings(QObject *parent) : QObject(parent) {
    load();
}

void SharedSettings::load() {
    QSettings settings(kOrg, kApp);
    m_darkMode = settings.value("darkMode", false).toBool();
    int accent = settings.value("accentColor", 0x4A90D9).toInt();
    m_accentColor = QColor::fromRgb(accent);
    m_themeName = settings.value("themeName", "system").toString();
}

void SharedSettings::save() {
    QSettings settings(kOrg, kApp);
    settings.setValue("darkMode", m_darkMode);
    settings.setValue("accentColor", m_accentColor.rgb());
    settings.setValue("themeName", m_themeName);
    settings.sync();
}

void SharedSettings::setDarkMode(bool isDark) {
    if (m_darkMode == isDark) return;
    m_darkMode = isDark;
    save();
    emit darkModeChanged();

    if (qApp) {
        auto *hints = qApp->styleHints();
        hints->setColorScheme(isDark ? Qt::ColorScheme::Dark : Qt::ColorScheme::Light);
    }
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
