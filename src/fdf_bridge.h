#ifndef FDF_BRIDGE_H
#define FDF_BRIDGE_H

#include <QObject>
#include <QCoreApplication>
#include <QString>
#include <QProcess>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QStringList>
#include <QMap>
#include <QByteArray>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonArray>
#include <QJsonValue>
#include <QDateTime>
#include <QUrl>
#include <QClipboard>
#include <QGuiApplication>
#include <QScreen>
#include <QJSValue>
#include <QJSEngine>
#include <QQmlEngine>
#include <QQmlContext>
#include <QThread>
#include <QMutex>
#include <QTimer>
#include <functional>
#include <cmath>
#include <QSet>
#include <QUuid>
#include <QRandomGenerator>
#include <QCryptographicHash>
#include <QMimeDatabase>
#include <QMimeType>
#include <QStandardPaths>
#include <QSysInfo>
#include <QFileSystemWatcher>


extern "C" {
    int rust_ffi_has(const char *name);
    char* rust_ffi_call(const char *name, const char *args_json);
    void rust_ffi_free_string(char *s);
    char* rust_ffi_list();
}

class FDFSettings : public QObject {
    Q_OBJECT
    Q_PROPERTY(QString filePath READ filePath NOTIFY filePathChanged)

public:
    explicit FDFSettings(QObject *parent = nullptr)
        : QObject(parent), m_dirty(false), m_saveTimer(this)
    {
        QString base = QDir::homePath() + "/.fdf";
        QDir().mkpath(base);
        m_path = base + "/settings.json";
        load();
        m_saveTimer.setSingleShot(true);
        m_saveTimer.setInterval(500);
        connect(&m_saveTimer, &QTimer::timeout, this, &FDFSettings::flush);
    }

    QString filePath() const { return m_path; }

    Q_INVOKABLE QVariant get(const QString &key, const QVariant &defaultValue = QVariant()) const {
        return m_data.value(key, defaultValue);
    }

    Q_INVOKABLE void set(const QString &key, const QVariant &value) {
        if (m_data.value(key) == value && m_data.contains(key)) return;
        m_data[key] = value;
        m_dirty = true;
        m_saveTimer.start();
        emit settingChanged(key, value);
    }

    Q_INVOKABLE bool has(const QString &key) const {
        return m_data.contains(key);
    }

    Q_INVOKABLE void remove(const QString &key) {
        if (!m_data.contains(key)) return;
        m_data.remove(key);
        m_dirty = true;
        m_saveTimer.start();
        emit settingChanged(key, QVariant());
    }

    Q_INVOKABLE void clear() {
        m_data.clear();
        m_dirty = true;
        m_saveTimer.start();
    }

    Q_INVOKABLE QVariantMap all() const {
        return m_data;
    }

    Q_INVOKABLE void flush() {
        if (!m_dirty) return;
        m_dirty = false;
        QJsonObject obj = QJsonObject::fromVariantMap(m_data);
        QFile f(m_path);
        if (f.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
            f.write(QJsonDocument(obj).toJson(QJsonDocument::Indented));
            f.close();
        }
    }

    Q_INVOKABLE void reload() {
        load();
    }

signals:
    void filePathChanged();
    void settingChanged(const QString &key, const QVariant &value);

private:
    void load() {
        QFile f(m_path);
        if (!f.open(QIODevice::ReadOnly)) return;
        QByteArray raw = f.readAll();
        f.close();
        QJsonParseError err;
        QJsonDocument doc = QJsonDocument::fromJson(raw, &err);
        if (err.error != QJsonParseError::NoError) return;
        if (!doc.isObject()) return;
        m_data = doc.object().toVariantMap();
    }

    QString m_path;
    QVariantMap m_data;
    bool m_dirty;
    QTimer m_saveTimer;
};

class FDFPlatform : public QObject {
    Q_OBJECT
    Q_PROPERTY(QString os READ os CONSTANT)
    Q_PROPERTY(QString osVersion READ osVersion CONSTANT)
    Q_PROPERTY(QString kernel READ kernel CONSTANT)
    Q_PROPERTY(QString arch READ arch CONSTANT)
    Q_PROPERTY(QString hostname READ hostname CONSTANT)
    Q_PROPERTY(QString homeDir READ homeDir CONSTANT)
    Q_PROPERTY(QString tempDir READ tempDir CONSTANT)
    Q_PROPERTY(QString desktop READ desktop CONSTANT)
    Q_PROPERTY(QString sessionType READ sessionType CONSTANT)
    Q_PROPERTY(int screenWidth READ screenWidth NOTIFY screenChanged)
    Q_PROPERTY(int screenHeight READ screenHeight NOTIFY screenChanged)
    Q_PROPERTY(qreal screenRatio READ screenRatio NOTIFY screenChanged)

public:
    explicit FDFPlatform(QObject *parent = nullptr) : QObject(parent) {
        m_desktop = QString::fromLocal8Bit(qgetenv("XDG_CURRENT_DESKTOP"));
        if (m_desktop.isEmpty()) m_desktop = QString::fromLocal8Bit(qgetenv("DESKTOP_SESSION"));
        m_sessionType = QString::fromLocal8Bit(qgetenv("XDG_SESSION_TYPE"));
        if (m_sessionType.isEmpty()) m_sessionType = "unknown";
        connect(qApp, &QGuiApplication::screenAdded, this, &FDFPlatform::screenChanged);
        connect(qApp, &QGuiApplication::screenRemoved, this, &FDFPlatform::screenChanged);
        connect(qApp, &QGuiApplication::primaryScreenChanged, this, [this]() {
            emit screenChanged();
        });
    }

    QString os() const { return QSysInfo::productType(); }
    QString osVersion() const { return QSysInfo::productVersion(); }
    QString kernel() const { return QSysInfo::kernelType() + " " + QSysInfo::kernelVersion(); }
    QString arch() const { return QSysInfo::currentCpuArchitecture(); }
    QString hostname() const { return QSysInfo::machineHostName(); }
    QString homeDir() const { return QDir::homePath(); }
    QString tempDir() const { return QStandardPaths::writableLocation(QStandardPaths::TempLocation); }
    QString desktop() const { return m_desktop; }
    QString sessionType() const { return m_sessionType; }

    int screenWidth() const {
        auto *s = QGuiApplication::primaryScreen();
        return s ? s->size().width() : 0;
    }

    int screenHeight() const {
        auto *s = QGuiApplication::primaryScreen();
        return s ? s->size().height() : 0;
    }

    qreal screenRatio() const {
        auto *s = QGuiApplication::primaryScreen();
        return s ? s->devicePixelRatio() : 1.0;
    }

    Q_INVOKABLE QStringList screenNames() const {
        QStringList names;
        for (auto *s : QGuiApplication::screens())
            names.append(s->name());
        return names;
    }

signals:
    void screenChanged();

private:
    QString m_desktop;
    QString m_sessionType;
};

class FDFFFI : public QObject {
    Q_OBJECT
    Q_PROPERTY(QStringList functions READ functions CONSTANT)

public:
    using FnPtr = std::function<QVariant(const QVariantList&)>;

    explicit FDFFFI(QObject *parent = nullptr) : QObject(parent) {
        registerBuiltins();
    }

    QStringList functions() const {
        return m_fns.keys();
    }

    Q_INVOKABLE QVariant call(const QString &name, const QVariantList &args = QVariantList()) {
        if (m_fns.contains(name))
            return m_fns[name](args);


        QByteArray nameUtf8 = name.toUtf8();
        QJsonArray argsJson;
        for (const auto &a : args) argsJson.append(QJsonValue::fromVariant(a));
        QByteArray argsUtf8 = QJsonDocument(argsJson).toJson(QJsonDocument::Compact);

        if (rust_ffi_has(nameUtf8.constData())) {
            char *result = rust_ffi_call(nameUtf8.constData(), argsUtf8.constData());
            QString resultStr = QString::fromUtf8(result);
            rust_ffi_free_string(result);
            return QVariant(resultStr);
        }

        qWarning("FDFFFI: unknown function '%s'", qPrintable(name));
        return QVariant();
    }

    Q_INVOKABLE int callAsync(const QString &name, const QVariantList &args,
                              QJSValue onResult, QJSValue onError = QJSValue())
    {
        if (!m_fns.contains(name)) {
            qWarning("FDFFFI: unknown async function '%s'", qPrintable(name));
            if (onError.isCallable())
                onError.call({QJSValue(QString("unknown function: %1").arg(name))});
            return -1;
        }
        int id = m_nextAsyncId++;
        auto it = m_fns.find(name);
        QMetaObject::invokeMethod(this, [this, id, name, args, onResult, onError]() {
            if (m_asyncCancelled.contains(id)) {
                m_asyncCancelled.remove(id);
                return;
            }
            QVariant result = call(name, args);
            m_asyncPending.remove(id);
            if (onResult.isCallable())
                onResult.call({QJSValue(qvariantToJs(result))});
        }, Qt::QueuedConnection);
        m_asyncPending.insert(id);
        return id;
    }

    Q_INVOKABLE void cancelAsync(int handle) {
        if (m_asyncPending.contains(handle)) {
            m_asyncCancelled.insert(handle);
        }
    }

    Q_INVOKABLE void registerFn(const QString &name, QJSValue fn) {
        if (!fn.isCallable()) {
            qWarning("FDFFFI: registerFn requires a callable argument");
            return;
        }
        m_jsFns[name] = QJSValue(fn);
        m_fns[name] = [this, name](const QVariantList &args) -> QVariant {
            auto it = m_jsFns.find(name);
            if (it == m_jsFns.end()) return QVariant();
            QJSValueList jsArgs;
            for (const auto &a : args) jsArgs.append(qvariantToJs(a));
            QJSValue result = it->call(jsArgs);
            if (result.isError()) {
                qWarning("FDFFFI: JS fn '%s' error: %s", qPrintable(name),
                         qPrintable(result.toString()));
                return QVariant();
            }
            return result.toVariant();
        };
        emit functionsChanged();
    }

    Q_INVOKABLE void unregisterFn(const QString &name) {
        m_fns.remove(name);
        m_jsFns.remove(name);
        emit functionsChanged();
    }

signals:
    void functionsChanged();

private:
    void registerBuiltins() {
        m_fns["uuid"] = [](const QVariantList &) -> QVariant {
            return QUuid::createUuid().toString(QUuid::WithoutBraces);
        };
        m_fns["timestamp"] = [](const QVariantList &) -> QVariant {
            return QDateTime::currentSecsSinceEpoch();
        };
        m_fns["timestampMs"] = [](const QVariantList &) -> QVariant {
            return QDateTime::currentMSecsSinceEpoch();
        };
        m_fns["dateStr"] = [](const QVariantList &args) -> QVariant {
            QString fmt = args.size() > 0 ? args[0].toString() : "yyyy-MM-dd";
            return QDateTime::currentDateTime().toString(fmt);
        };
        m_fns["env"] = [](const QVariantList &args) -> QVariant {
            if (args.isEmpty()) return QVariant();
            return QString::fromLocal8Bit(qgetenv(args[0].toString().toLocal8Bit()));
        };
        m_fns["sleep"] = [](const QVariantList &args) -> QVariant {
            int ms = args.size() > 0 ? qMax(0, args[0].toInt()) : 0;
            QThread::msleep(ms);
            return QVariant();
        };
        m_fns["typeOf"] = [](const QVariantList &args) -> QVariant {
            if (args.isEmpty()) return QString("undefined");
            switch (args[0].type()) {
                case QVariant::Bool: return QString("boolean");
                case QVariant::Int:
                case QVariant::Double:
                case QVariant::LongLong:
                case QVariant::ULongLong: return QString("number");
                case QVariant::String: return QString("string");
                case QVariant::List: return QString("array");
                case QVariant::Map: return QString("object");
                default: return QString("unknown");
            }
        };
        m_fns["debug"] = [](const QVariantList &args) -> QVariant {
            for (const auto &a : args)
                qDebug() << "[ffi]" << a;
            return QVariant();
        };
        m_fns["urlEncode"] = [](const QVariantList &args) -> QVariant {
            if (args.isEmpty()) return QVariant();
            return QUrl::toPercentEncoding(args[0].toString());
        };
        m_fns["urlDecode"] = [](const QVariantList &args) -> QVariant {
            if (args.isEmpty()) return QVariant();
            return QUrl::fromPercentEncoding(args[0].toString().toUtf8());
        };
        m_fns["base64encode"] = [](const QVariantList &args) -> QVariant {
            if (args.isEmpty()) return QVariant();
            return args[0].toString().toUtf8().toBase64();
        };
        m_fns["base64decode"] = [](const QVariantList &args) -> QVariant {
            if (args.isEmpty()) return QVariant();
            return QString::fromUtf8(QByteArray::fromBase64(args[0].toString().toUtf8()));
        };
        m_fns["md5"] = [](const QVariantList &args) -> QVariant {
            if (args.isEmpty()) return QVariant();
            return QString(QCryptographicHash::hash(args[0].toString().toUtf8(), QCryptographicHash::Md5).toHex());
        };
        m_fns["sha256"] = [](const QVariantList &args) -> QVariant {
            if (args.isEmpty()) return QVariant();
            return QString(QCryptographicHash::hash(args[0].toString().toUtf8(), QCryptographicHash::Sha256).toHex());
        };
        m_fns["random"] = [](const QVariantList &args) -> QVariant {
            int min = args.size() > 0 ? args[0].toInt() : 0;
            int max = args.size() > 1 ? args[1].toInt() : RAND_MAX;
            return QVariant(QRandomGenerator::global()->bounded(min, max + 1));
        };
        m_fns["randomFloat"] = [](const QVariantList &) -> QVariant {
            return QVariant(QRandomGenerator::global()->generateDouble());
        };
        m_fns["round"] = [](const QVariantList &args) -> QVariant {
            if (args.isEmpty()) return QVariant();
            int decimals = args.size() > 1 ? args[1].toInt() : 0;
            double val = args[0].toDouble();
            double factor = std::pow(10.0, decimals);
            return QVariant(std::round(val * factor) / factor);
        };
        m_fns["clamp"] = [](const QVariantList &args) -> QVariant {
            if (args.size() < 3) return QVariant();
            double val = args[0].toDouble();
            double lo = args[1].toDouble();
            double hi = args[2].toDouble();
            return QVariant(qBound(lo, val, hi));
        };
        m_fns["lerp"] = [](const QVariantList &args) -> QVariant {
            if (args.size() < 3) return QVariant();
            double a = args[0].toDouble();
            double b = args[1].toDouble();
            double t = args[2].toDouble();
            return QVariant(a + (b - a) * t);
        };
        m_fns["format"] = [](const QVariantList &args) -> QVariant {
            if (args.size() < 2) return QVariant();
            QString fmt = args[0].toString();
            QStringList parts;
            for (int i = 1; i < args.size(); i++)
                parts << args[i].toString();
            return fmt.arg(parts[0], parts.size() > 1 ? parts[1] : QString());
        };
        m_fns["jsonParse"] = [](const QVariantList &args) -> QVariant {
            if (args.isEmpty()) return QVariant();
            QJsonParseError err;
            QJsonDocument doc = QJsonDocument::fromJson(args[0].toString().toUtf8(), &err);
            if (err.error != QJsonParseError::NoError) return QVariant();
            return doc.toVariant();
        };
        m_fns["jsonStringify"] = [](const QVariantList &args) -> QVariant {
            if (args.isEmpty()) return QVariant();
            QJsonDocument doc = QJsonDocument::fromVariant(args[0]);
            return QString::fromUtf8(doc.toJson(QJsonDocument::Compact));
        };
        m_fns["platform"] = [](const QVariantList &) -> QVariant {
            QVariantMap info;
            info["os"] = QSysInfo::productType();
            info["kernel"] = QSysInfo::kernelType();
            info["arch"] = QSysInfo::currentCpuArchitecture();
            info["hostname"] = QSysInfo::machineHostName();
            info["desktop"] = qEnvironmentVariable("XDG_CURRENT_DESKTOP");
            return info;
        };
        m_fns["locale"] = [](const QVariantList &) -> QVariant {
            return QLocale().name();
        };
        m_fns["timezone"] = [](const QVariantList &) -> QVariant {
            return QDateTime::currentDateTime().timeZoneAbbreviation();
        };
    }

    QJSValue qvariantToJs(const QVariant &v) {
        QJSEngine *engine = QQmlEngine::contextForObject(this)->engine();
        return engine ? engine->toScriptValue(v) : QJSValue();
    }

    QMap<QString, FnPtr> m_fns;
    QMap<QString, QJSValue> m_jsFns;
    QSet<int> m_asyncPending;
    QSet<int> m_asyncCancelled;
    int m_nextAsyncId = 1;
};

class FDFClipboard : public QObject {
    Q_OBJECT
    Q_PROPERTY(QString text READ text WRITE setText NOTIFY textChanged)
    Q_PROPERTY(QString selection READ selection WRITE setSelection NOTIFY selectionChanged)

public:
    explicit FDFClipboard(QObject *parent = nullptr) : QObject(parent) {
        auto *clip = QGuiApplication::clipboard();
        connect(clip, &QClipboard::dataChanged, this, &FDFClipboard::textChanged);
        connect(clip, &QClipboard::selectionChanged, this, [this]() {
            emit selectionChanged();
        });
    }

    QString text() const {
        return QGuiApplication::clipboard()->text();
    }

    void setText(const QString &t) {
        QGuiApplication::clipboard()->setText(t);
    }

    QString selection() const {
        return QGuiApplication::clipboard()->text(QClipboard::Selection);
    }

    void setSelection(const QString &s) {
        QGuiApplication::clipboard()->setText(s, QClipboard::Selection);
    }

    Q_INVOKABLE void clear() {
        QGuiApplication::clipboard()->clear();
    }

    Q_INVOKABLE QString getText() const { return text(); }

    Q_INVOKABLE void copy(const QString &text) { setText(text); }

signals:
    void textChanged();
    void selectionChanged();
};

extern QString g_fdfQmxPath;
extern QString g_fdfOutputPath;
extern QString g_fdfFallback;

class FDFBridge : public QObject {
    Q_OBJECT
private:
    QMap<int, QProcess*> m_processes;
    int m_nextHandle = 1;
    QFileSystemWatcher *m_watcher = nullptr;
    QTimer *m_reloadDebounce = nullptr;

    void startWatchingQmx() {
        if (m_watcher || g_fdfQmxPath.isEmpty()) return;
        QFileInfo fi(g_fdfQmxPath);
        if (!fi.exists()) return;
        if (!fi.isWritable() && !QFileInfo(fi.absolutePath()).isWritable()) {
            return;
        }
        m_watcher = new QFileSystemWatcher(this);
        if (!m_watcher->addPath(g_fdfQmxPath)) {
            delete m_watcher;
            m_watcher = nullptr;
            return;
        }
        m_reloadDebounce = new QTimer(this);
        m_reloadDebounce->setSingleShot(true);
        m_reloadDebounce->setInterval(300);
        QObject::connect(m_reloadDebounce, &QTimer::timeout, this, [this]() {
            doQmxTranspile();
        });
        QObject::connect(m_watcher, &QFileSystemWatcher::fileChanged, this, [this](const QString &) {
            m_reloadDebounce->start();
            if (g_fdfQmxPath.isEmpty() || !QFileInfo::exists(g_fdfQmxPath)) return;
            if (!m_watcher->files().contains(g_fdfQmxPath))
                m_watcher->addPath(g_fdfQmxPath);
        });
    }

    void doQmxTranspile() {
        if (g_fdfQmxPath.isEmpty() || g_fdfOutputPath.isEmpty()) return;
        QProcess proc;
        proc.start("qmx-transpile", QStringList() << g_fdfQmxPath << g_fdfOutputPath);
        proc.waitForFinished(30000);
        if (proc.exitCode() == 0) {
            QFileInfo fi(g_fdfOutputPath);
            emit qmxReloaded(fi.lastModified().toString(Qt::ISODate));
        } else {
            QString err = QString::fromUtf8(proc.readAllStandardError());
            emit qmxError(err);
        }
    }

public:
    FDFBridge(QObject *parent = nullptr) : QObject(parent) {
        if (!g_fdfQmxPath.isEmpty()) startWatchingQmx();
    }

    Q_INVOKABLE QString runProcessSync(const QString &command) {
        QProcess proc;
        proc.start("sh", QStringList() << "-c" << command);
        proc.waitForFinished(60000);
        QByteArray out = proc.readAllStandardOutput();
        QByteArray err = proc.readAllStandardError();
        if (!err.isEmpty()) {
            if (!out.isEmpty()) out.append('\n');
            out.append(err);
        }
        return QString::fromUtf8(out);
    }

    Q_INVOKABLE QString homeDir() {
        return QDir::homePath();
    }

    Q_INVOKABLE QString fdfDir() {
        return QDir::homePath() + "/.fdf";
    }

    Q_INVOKABLE void execDetached(const QString &command) {
        QProcess::startDetached("sh", QStringList() << "-c" << command);
    }

    Q_INVOKABLE QString readFile(const QString &path) {
        QFile f(path);
        if (!f.open(QIODevice::ReadOnly)) return QString();
        return QString::fromUtf8(f.readAll());
    }

    Q_INVOKABLE bool writeFile(const QString &path, const QString &content) {
        QDir().mkpath(QFileInfo(path).absolutePath());
        QFile f(path);
        if (!f.open(QIODevice::WriteOnly)) return false;
        f.write(content.toUtf8());
        f.close();
        return true;
    }

    Q_INVOKABLE bool appendFile(const QString &path, const QString &content) {
        QDir().mkpath(QFileInfo(path).absolutePath());
        QFile f(path);
        if (!f.open(QIODevice::Append)) return false;
        f.write(content.toUtf8());
        f.close();
        return true;
    }

    Q_INVOKABLE bool removeFile(const QString &path) {
        return QFile::remove(path);
    }

    Q_INVOKABLE bool fileExists(const QString &path) {
        return QFileInfo::exists(path);
    }

    Q_INVOKABLE QStringList listDir(const QString &path) {
        QDir d(path);
        if (!d.exists()) return {};
        return d.entryList(QDir::NoDotAndDotDot | QDir::Files | QDir::Dirs);
    }

    Q_INVOKABLE QString shellPath(const QString &name) {
        QString base = qEnvironmentVariable("FDF_SCRIPTS");
        if (base.isEmpty())
            base = QCoreApplication::applicationDirPath();
        return base + "/" + name;
    }

    Q_INVOKABLE void fifoWrite(const QString &icon, const QString &title, const QString &body) {
        QString home = qEnvironmentVariable("HOME");
        if (home.isEmpty()) return;
        QString fifo = home + "/.fdf/fifos/qs_notify";
        QDir().mkpath(QFileInfo(fifo).absolutePath());
        QFile f(fifo);
        if (f.open(QIODevice::WriteOnly)) {
            QString t = title;
            QString b = body;
            QByteArray msg = QString("%1|%2|%3|notif\n")
                .arg(icon, t.replace('\'', "\\'"), b.replace('\'', "\\'"))
                .toUtf8();
            f.write(msg);
            f.close();
        }
    }

    Q_INVOKABLE int startProcessAsync(const QString &command) {
        int handle = m_nextHandle++;
        QProcess *proc = new QProcess(this);
        m_processes[handle] = proc;
        QObject::connect(proc, QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished),
            this, [this, handle](int exitCode, QProcess::ExitStatus ) {
            QProcess *p = m_processes.take(handle);
            if (!p) return;
            QString output = QString::fromUtf8(p->readAllStandardOutput());
            QByteArray err = p->readAllStandardError();
            if (!err.isEmpty()) {
                if (!output.isEmpty()) output.append('\n');
                output.append(QString::fromUtf8(err));
            }
            emit processFinished(handle, output, exitCode);
            p->deleteLater();
        });
        proc->start("sh", QStringList() << "-c" << command);
        return handle;
    }

    Q_INVOKABLE QString joinPath(const QString &a, const QString &b) {
        return QDir(a).absoluteFilePath(b);
    }

    Q_INVOKABLE QString dirName(const QString &path) {
        return QFileInfo(path).dir().dirName();
    }

    Q_INVOKABLE QString baseName(const QString &path) {
        return QFileInfo(path).baseName();
    }

    Q_INVOKABLE QString absolutePath(const QString &path) {
        return QFileInfo(path).absolutePath();
    }

    Q_INVOKABLE QString mimeType(const QString &path) {
        QMimeDatabase db;
        return db.mimeTypeForFile(path).name();
    }

    Q_INVOKABLE QVariantMap fileInfo(const QString &path) {
        QFileInfo fi(path);
        QVariantMap info;
        info["exists"] = fi.exists();
        info["size"] = fi.size();
        info["isDir"] = fi.isDir();
        info["isFile"] = fi.isFile();
        info["isReadable"] = fi.isReadable();
        info["isWritable"] = fi.isWritable();
        info["isExecutable"] = fi.isExecutable();
        info["lastModified"] = fi.lastModified().toString(Qt::ISODate);
        info["lastAccessed"] = fi.lastRead().toString(Qt::ISODate);
        info["suffix"] = fi.suffix();
        info["baseName"] = fi.baseName();
        info["fileName"] = fi.fileName();
        info["absolutePath"] = fi.absolutePath();
        return info;
    }

    Q_INVOKABLE QString xdgIconPath(const QString &name) {
        if (name.isEmpty()) return {};
        QStringList dirs = {
            QDir::homePath() + "/.icons",
            QDir::homePath() + "/.local/share/icons",
            "/usr/share/icons",
            "/usr/local/share/icons"
        };

        QStringList dataDirs = QString::fromLocal8Bit(qgetenv("XDG_DATA_DIRS"))
            .split(':', Qt::SkipEmptyParts);
        for (const auto &d : dataDirs) {
            QString candidate = d + "/icons";
            if (!dirs.contains(candidate)) dirs.append(candidate);
        }
        if (!dirs.contains("/usr/share/icons")) dirs.append("/usr/share/icons");

        QStringList exts = {".svg", ".svgz", ".png", ".xpm"};
        QStringList categories = {"apps", "actions", "places", "devices", "mimetypes", "status", "emblems"};
        QString base = name;

        for (const auto &theme : {"hicolor", "Adwaita", "Papirus", "gnome", "Breeze", "Numix"}) {
            for (const auto &dir : dirs) {
                QString themeDir = dir + "/" + theme;
                if (!QDir(themeDir).exists()) continue;

                for (const auto &cat : categories) {
                    for (const auto &ext : exts) {
                        QString path = themeDir + "/scalable/" + cat + "/" + base + ext;
                        if (QFileInfo::exists(path)) return path;
                    }
                }

                QStringList sizes = {"48x48", "32x32", "24x24", "22x22", "16x16", "64x64"};
                for (const auto &size : sizes) {
                    for (const auto &cat : categories) {
                        for (const auto &ext : exts) {
                            QString path = themeDir + "/" + size + "/" + cat + "/" + base + ext;
                            if (QFileInfo::exists(path)) return path;
                        }
                    }
                }

                for (const auto &ext : exts) {
                    QString path = themeDir + "/" + base + ext;
                    if (QFileInfo::exists(path)) return path;
                }
            }
        }

        if (QFileInfo::exists(name)) return name;

        for (const auto &dir : dirs) {
            QString hicolor = dir + "/hicolor";
            if (!QDir(hicolor).exists()) continue;
            for (const auto &cat : categories) {
                for (const auto &ext : exts) {
                    QString path = hicolor + "/scalable/" + cat + "/" + base + ext;
                    if (QFileInfo::exists(path)) return path;
                }
            }
        }
        return {};
    }

    Q_INVOKABLE QVariantList listFiles(const QString &path, const QString &filter = "*") {
        QDir d(path);
        if (!d.exists()) return {};
        QVariantList results;
        for (const auto &fi : d.entryInfoList({filter}, QDir::Files | QDir::Dirs | QDir::NoDotAndDotDot)) {
            QVariantMap entry;
            entry["name"] = fi.fileName();
            entry["path"] = fi.absoluteFilePath();
            entry["isDir"] = fi.isDir();
            entry["size"] = fi.size();
            entry["lastModified"] = fi.lastModified().toString(Qt::ISODate);
            results.append(entry);
        }
        return results;
    }

    Q_INVOKABLE bool qmxReload() {
        doQmxTranspile();
        return true;
    }

    Q_INVOKABLE void qmxWatch() {
        startWatchingQmx();
    }

    Q_INVOKABLE QString qmxPath() const { return g_fdfQmxPath; }
    Q_INVOKABLE QString qmxOutputPath() const { return g_fdfOutputPath; }

signals:
    void processFinished(int handle, const QString &output, int exitCode);
    void qmxReloaded(const QString &timestamp);
    void qmxError(const QString &message);
};

class FDFIPC;

#include <QLocalServer>
#include <QLocalSocket>
#include <QJsonDocument>
#include <QJsonObject>

class FDFIPC : public QObject {
    Q_OBJECT
    Q_PROPERTY(bool running READ isRunning NOTIFY runningChanged)

public:
    explicit FDFIPC(QObject *parent = nullptr)
        : QObject(parent), m_server(new QLocalServer(this)), m_nextId(0)
    {
        QString sockPath = QDir::homePath() + "/.fdf/ipc.sock";
        QLocalServer::removeServer(sockPath);
        QDir().mkpath(QFileInfo(sockPath).absolutePath());

        if (m_server->listen(sockPath)) {
            qDebug() << "FDFIPC listening on" << sockPath;
            QObject::connect(m_server, &QLocalServer::newConnection, this, [this]() {
                while (m_server->hasPendingConnections()) {
                    QLocalSocket *client = m_server->nextPendingConnection();
                    int id = m_nextId++;
                    m_clients[id] = client;
                    QObject::connect(client, &QLocalSocket::readyRead, this, [this, id, client]() {
                        QByteArray data = client->readAll();
                        QJsonParseError err;
                        QJsonDocument doc = QJsonDocument::fromJson(data, &err);
                        if (err.error == QJsonParseError::NoError && doc.isObject()) {
                            QJsonObject msg = doc.object();
                            QString type = msg["type"].toString();
                            QVariant payload = msg["data"].toVariant();
                            emit messageReceived(type, payload, id);
                        }
                    });
                    QObject::connect(client, &QLocalSocket::disconnected, this, [this, id, client]() {
                        m_clients.remove(id);
                        client->deleteLater();
                    });
                }
            });
        } else {
            qWarning() << "FDFIPC failed to listen:" << m_server->errorString();
        }
    }

    bool isRunning() const { return m_server->isListening(); }

    Q_INVOKABLE bool send(const QString &type, const QVariant &data = QVariant()) {
        QJsonObject msg;
        msg["type"] = type;
        msg["data"] = QJsonValue::fromVariant(data);
        QByteArray payload = QJsonDocument(msg).toJson(QJsonDocument::Compact) + "\n";

        bool sent = false;
        for (auto it = m_clients.begin(); it != m_clients.end();) {
            if (it.value()->state() == QLocalSocket::ConnectedState) {
                it.value()->write(payload);
                it.value()->flush();
                sent = true;
                ++it;
            } else {
                it.value()->deleteLater();
                it = m_clients.erase(it);
            }
        }
        return sent;
    }

    Q_INVOKABLE void broadcast(const QString &type, const QVariant &data = QVariant()) {
        send(type, data);
        emit messageReceived(type, data, -1);
    }

signals:
    void messageReceived(const QString &type, const QVariant &data, int senderId);
    void runningChanged();

private:
    QLocalServer *m_server;
    QMap<int, QLocalSocket*> m_clients;
    int m_nextId;
};

#endif 
