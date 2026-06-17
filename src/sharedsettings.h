#ifndef SHAREDSETTINGS_H
#define SHAREDSETTINGS_H

#include <QObject>
#include <QColor>

class SharedSettings : public QObject {
    Q_OBJECT
    Q_PROPERTY(bool darkMode READ darkMode WRITE setDarkMode NOTIFY darkModeChanged)
    Q_PROPERTY(QColor accentColor READ accentColor WRITE setAccentColor NOTIFY accentColorChanged)
    Q_PROPERTY(QString themeName READ themeName WRITE setThemeName NOTIFY themeNameChanged)

public:
    explicit SharedSettings(QObject *parent = nullptr);

    bool darkMode() const { return m_darkMode; }
    QColor accentColor() const { return m_accentColor; }
    QString themeName() const { return m_themeName; }

    void setDarkMode(bool isDark);
    void setAccentColor(const QColor &color);
    void setThemeName(const QString &name);

signals:
    void darkModeChanged();
    void accentColorChanged();
    void themeNameChanged();

private:
    void load();
    void save();

    bool m_darkMode = false;
    QColor m_accentColor;
    QString m_themeName;
};

#endif
