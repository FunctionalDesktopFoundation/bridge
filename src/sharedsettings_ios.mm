#include "sharedsettings.h"
#import <Foundation/Foundation.h>

static NSUserDefaults *defaults() {
    return [NSUserDefaults standardUserDefaults];
}

SharedSettings::SharedSettings(QObject *parent) : QObject(parent) {
    load();
}

void SharedSettings::load() {
    m_darkMode = [defaults() boolForKey:@"fdf_darkMode"];

    int accent = (int)[defaults() integerForKey:@"fdf_accentColor"];
    m_accentColor = accent ? QColor::fromRgb(accent) : QColor(0x4A, 0x90, 0xD9);

    NSString *theme = [defaults() stringForKey:@"fdf_themeName"];
    m_themeName = theme ? QString::fromNSString(theme) : QStringLiteral("system");
}

void SharedSettings::save() {
    [defaults() setBool:m_darkMode forKey:@"fdf_darkMode"];
    [defaults() setInteger:m_accentColor.rgb() forKey:@"fdf_accentColor"];
    [defaults() setObject:m_themeName.toNSString() forKey:@"fdf_themeName"];
    [defaults() synchronize];
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
