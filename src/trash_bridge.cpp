#include "trash_bridge.h"
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

QString g_qmxPath;
QString g_qmlOutputPath;
QString g_qmlFallback;

static QString resolveQmlPath() {
    QString qmlPath = QString::fromLocal8Bit(qgetenv("TRASH_QML"));
    if (qmlPath.isEmpty())
        qmlPath = QCoreApplication::applicationDirPath() + "/shell.qml";

    QFileInfo qmlInfo(qmlPath);
    QString baseDir = qmlInfo.absolutePath();
    QString baseName = qmlInfo.completeBaseName();

    g_qmlFallback = qmlPath;

    if (qmlInfo.suffix().toLower() == "qmx") {
        g_qmxPath = qmlPath;
        QString appName = QFileInfo(baseDir).fileName();
        QString tmpDir = QStandardPaths::writableLocation(QStandardPaths::TempLocation) + "/fdf-qmx";
        QDir().mkpath(tmpDir);
        g_qmlOutputPath = tmpDir + "/" + appName + "_" + baseName + ".qml";

        QProcess proc;
        proc.start("qmx-transpile", QStringList() << g_qmxPath << g_qmlOutputPath);
        proc.waitForFinished(30000);
        if (proc.exitCode() != 0) {
            QString err = QString::fromUtf8(proc.readAllStandardError());
            qWarning() << "QMX transpilation failed:" << err << "falling back to QML";
            return g_qmlFallback;
        }

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

        return g_qmlOutputPath;
    }

    return qmlPath;
}

extern "C" int run_trash_app(void) {
    int argc = 1;
    char *argv[2] = { const_cast<char*>("trash-app"), nullptr };
    QGuiApplication app(argc, argv);

    QString qmlPath = resolveQmlPath();

    QFileInfo qmlInfo(qmlPath);
    QString appShareDir = qmlInfo.absolutePath();
    loadBundledFonts(appShareDir + "/fonts");

    QQmlApplicationEngine engine;

    TrashBridge bridge;
    engine.rootContext()->setContextProperty("bridge", &bridge);

    TrashSettings settings;
    engine.rootContext()->setContextProperty("TrashSettings", &settings);

    TrashPlatform platform;
    engine.rootContext()->setContextProperty("TrashPlatform", &platform);

    TrashFFI ffi;
    engine.rootContext()->setContextProperty("TrashFFI", &ffi);

    TrashClipboard clipboard;
    engine.rootContext()->setContextProperty("TrashClipboard", &clipboard);

    FDFIPC ipc;
    engine.rootContext()->setContextProperty("FDFIPC", &ipc);

    QUrl url = QUrl::fromLocalFile(qmlPath);
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed,
        &app, [&]() {
        qWarning("Failed to load QML: %s", qPrintable(url.toString()));
        QCoreApplication::exit(1);
    });

    engine.load(url);

    return app.exec();
}
