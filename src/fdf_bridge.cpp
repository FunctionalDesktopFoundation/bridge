#include "fdf_bridge.h"
#include "sharedsettings.h"
#include "hooks_config.h"
#include <QColor>
#include <QProcess>
#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QUrl>
#include <QString>
#include <QByteArray>
#include <QFontDatabase>
#include <QDirIterator>
#include <QFile>
#include <QFileInfo>
#include <QStandardPaths>
#include <QDebug>

static void loadBundledFonts(const QString &fontDir) {
    if (!QDir(fontDir).exists()) return;
    QDirIterator it(fontDir, QDir::Files, QDirIterator::Subdirectories);
    int loaded = 0;
    while (it.hasNext()) {
        it.next();
        QString path = it.filePath();
        if (path.endsWith(".ttf", Qt::CaseInsensitive) || path.endsWith(".otf", Qt::CaseInsensitive)) {
            int id = QFontDatabase::addApplicationFont(path);
            if (id >= 0) loaded++;
        }
    }
    if (loaded > 0)
        qDebug() << "Loaded" << loaded << "font files from" << fontDir;
}

static void loadBundledFontsFromQrc() {
    QDirIterator it(":", QDir::Files, QDirIterator::Subdirectories);
    int loaded = 0;
    while (it.hasNext()) {
        it.next();
        QString path = it.filePath();
        if (path.endsWith(".ttf", Qt::CaseInsensitive) || path.endsWith(".otf", Qt::CaseInsensitive)) {
            int id = QFontDatabase::addApplicationFont(path);
            if (id >= 0) loaded++;
        }
    }
    if (loaded > 0)
        qDebug() << "Loaded" << loaded << "font files from Qt resources";
}

static void platformDetect(QQmlApplicationEngine &engine);

static QUrl resolveQmlUrl(QQmlApplicationEngine &engine) {
    QString fdfQml = QString::fromLocal8Bit(qgetenv("FDF_QML"));
    if (!fdfQml.isEmpty()) {
        QFileInfo fi(fdfQml);
        if (fi.exists()) {
            QString baseDir = fi.absolutePath();
            for (const auto &path : {baseDir + "/FDF", baseDir + "/stdlib", baseDir}) {
                if (QDir(path).exists())
                    engine.addImportPath(path);
            }
            return QUrl::fromLocalFile(fdfQml);
        }
    }
    if (QFile::exists(":/shell.qml")) {
        engine.addImportPath("qrc:///");
        return QUrl("qrc:///shell.qml");
    }
    QString fsPath = QCoreApplication::applicationDirPath() + "/shell.qml";
    if (QFileInfo::exists(fsPath)) {
        QString baseDir = QFileInfo(fsPath).absolutePath();
        for (const auto &path : {baseDir + "/FDF", baseDir + "/stdlib", baseDir}) {
            if (QDir(path).exists())
                engine.addImportPath(path);
        }
        return QUrl::fromLocalFile(fsPath);
    }
    engine.addImportPath("qrc:///");
    return QUrl("qrc:///shell.qml");
}

extern "C" int run_fdf_app(void) {
    int argc = 1;
    char *argv[2] = { const_cast<char*>("fdf-app"), nullptr };
    QGuiApplication app(argc, argv);

    QQmlApplicationEngine engine;

    loadBundledFontsFromQrc();
    loadBundledFonts(QCoreApplication::applicationDirPath() + "/fonts");

    FDFBridge bridge;
    engine.rootContext()->setContextProperty("bridge", &bridge);

    FDFSettings settings;
    engine.rootContext()->setContextProperty("FDFSettings", &settings);

    FDFPlatform platform;
    engine.rootContext()->setContextProperty("FDFPlatform", &platform);

    FDFFFI ffi;
    engine.rootContext()->setContextProperty("FDFFFI", &ffi);

    FDFClipboard clipboard;
    engine.rootContext()->setContextProperty("FDFClipboard", &clipboard);

    FDFIPC ipc;
    engine.rootContext()->setContextProperty("FDFIPC", &ipc);

    FDFHooks hooks(g_hooks, g_hookCount);
    engine.rootContext()->setContextProperty("FDFHooks", &hooks);

    platformDetect(engine);

    SharedSettings sharedSettings;
    engine.rootContext()->setContextProperty("SharedSettings", &sharedSettings);
    engine.rootContext()->setContextProperty("FDF_DARK_MODE", sharedSettings.darkMode());
    engine.rootContext()->setContextProperty("FDF_ACCENT_COLOR", sharedSettings.accentColor());
    engine.rootContext()->setContextProperty("FDF_THEME_NAME", sharedSettings.themeName());

    QUrl url = resolveQmlUrl(engine);
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed,
        &app, [&]() {
        qWarning("Failed to load QML: %s", qPrintable(url.toString()));
        QCoreApplication::exit(1);
    });

    engine.load(url);
    return app.exec();
}

static void platformDetect(QQmlApplicationEngine &engine) {
#if defined(Q_OS_WIN)
    engine.rootContext()->setContextProperty("FDF_IS_MOBILE", false);
    engine.rootContext()->setContextProperty("FDF_HAVE_WINDOW_CONTROLS", false);
    engine.rootContext()->setContextProperty("FDF_PLATFORM", "windows");
    engine.rootContext()->setContextProperty("FDF_TOUCH_TARGET", 32);
#elif defined(Q_OS_MACOS)
    engine.rootContext()->setContextProperty("FDF_IS_MOBILE", false);
    engine.rootContext()->setContextProperty("FDF_HAVE_WINDOW_CONTROLS", false);
    engine.rootContext()->setContextProperty("FDF_PLATFORM", "macos");
    engine.rootContext()->setContextProperty("FDF_TOUCH_TARGET", 32);
#else
    engine.rootContext()->setContextProperty("FDF_IS_MOBILE", false);
    engine.rootContext()->setContextProperty("FDF_HAVE_WINDOW_CONTROLS", true);
    engine.rootContext()->setContextProperty("FDF_PLATFORM", "desktop");
    engine.rootContext()->setContextProperty("FDF_TOUCH_TARGET", 32);
#endif
}
