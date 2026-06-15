#include "fdf_bridge.h"
#include <QProcess>
#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QUrl>
#include <QString>
#include <QByteArray>
#include <QFontDatabase>
#include <QDirIterator>
#include <QFileInfo>
#include <QStandardPaths>
#include <QDebug>
#include <QResource>

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

static void loadBundledFontsFromQrc(const QString &qrcPrefix) {
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

QString g_fdfQmxPath;
QString g_fdfOutputPath;
QString g_fdfFallback;

static QString resolveQmlPath() {
    QString qmlPath = QString::fromLocal8Bit(qgetenv("FDF_QML"));
    if (!qmlPath.isEmpty()) {
        QFileInfo qmlInfo(qmlPath);
        if (qmlInfo.exists())
            return qmlPath;
    }


    if (QResource::registerResource(QCoreApplication::applicationDirPath() + "/resources.rcc")) {
        qDebug() << "Loaded bundled Qt resources";
    }
    QUrl qrcUrl("qrc:///shell.qml");
    if (QFile::exists(":/shell.qml")) {
        return "qrc:///shell.qml";
    }


    QString fsPath = QCoreApplication::applicationDirPath() + "/shell.qml";
    if (QFileInfo::exists(fsPath))
        return fsPath;


    qmlPath = QString::fromLocal8Bit(qgetenv("FDF_QML"));
    if (!qmlPath.isEmpty()) {
        QFileInfo qmlInfo(qmlPath);
        QString baseDir = qmlInfo.absolutePath();
        QString baseName = qmlInfo.completeBaseName();

        if (qmlInfo.suffix().toLower() == "qmx") {
            g_fdfQmxPath = qmlPath;
            QString appName = QFileInfo(baseDir).fileName();
            QString tmpDir = QStandardPaths::writableLocation(QStandardPaths::TempLocation) + "/fdf-qmx";
            QDir().mkpath(tmpDir);
            g_fdfOutputPath = tmpDir + "/" + appName + "_" + baseName + ".qml";

            QProcess proc;
            proc.start("qmx-transpile", QStringList() << g_fdfQmxPath << g_fdfOutputPath);
            proc.waitForFinished(30000);
            if (proc.exitCode() == 0) {
                auto copyDir = [](const QString &src, const QString &dst) {
                    if (!QDir(src).exists()) return;
                    QDir().mkpath(dst);
                    QDirIterator it(src, QDir::Files, QDirIterator::NoIteratorFlags);
                    while (it.hasNext()) {
                        it.next();
                        QFile::copy(it.filePath(), dst + "/" + it.fileName());
                    }
                };
                copyDir(baseDir + "/FDF", tmpDir + "/FDF");
                copyDir(baseDir + "/stdlib", tmpDir + "/stdlib");
                return g_fdfOutputPath;
            }
        }
    }

    return fsPath;
}

extern "C" int run_fdf_app(void) {
    int argc = 1;
    char *argv[2] = { const_cast<char*>("fdf-app"), nullptr };
    QGuiApplication app(argc, argv);

    QString qmlPath = resolveQmlPath();
    bool usingQrc = qmlPath.startsWith("qrc://");

    QQmlApplicationEngine engine;


    engine.addImportPath("qrc:///");


    if (!usingQrc) {
        QFileInfo qmlInfo(qmlPath);
        QString appShareDir = qmlInfo.absolutePath();
        engine.addImportPath(appShareDir);
        loadBundledFonts(appShareDir + "/fonts");
    } else {
        loadBundledFontsFromQrc("qrc:///fonts");
    }

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

    QUrl url = usingQrc ? QUrl(qmlPath) : QUrl::fromLocalFile(qmlPath);
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed,
        &app, [&]() {
        qWarning("Failed to load QML: %s", qPrintable(url.toString()));
        QCoreApplication::exit(1);
    });

    engine.load(url);
    return app.exec();
}
