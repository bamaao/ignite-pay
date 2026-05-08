# Windows 下 Android 交叉编译 OpenSSL 构建问题修复指南

> 适用环境：Windows + Flutter + flutter_rust_bridge + Solana SDK
> 问题日期：2025-05

---

## 问题现象

在 Windows 上执行 `flutter build apk` 构建 Flutter Android 应用后，安装到手机上打开应用，页面停留在启动画面（白屏或 logo 页），无法进入主界面。

## 根本原因

### 1. Rust 原生库未被打包进 APK

通过检查 APK 内容发现，`librust_lib_ignite_pay_app.so`（Rust 编译产物）**完全不存在**于任何架构的 APK 中。APK 只包含 Flutter 引擎和其他 Flutter 插件的 .so 文件，但不包含应用自己的 Rust 库。

### 2. openssl-sys 交叉编译失败

Cargokit（flutter_rust_bridge 的构建系统）在为 Android 目标编译 Rust crate 时，`openssl-sys` 构建失败。`openssl-sys` 是 Solana SDK 的传递依赖：

```
solana-sdk → solana-precompiles → solana-secp256r1-program → openssl → openssl-sys
```

在 Windows 上，`openssl-sys` 构建失败有两个具体原因：

#### 原因 A：系统 OpenSSL 环境变量冲突

Windows 系统设置了 `OPENSSL_LIB_DIR` 和 `OPENSSL_INCLUDE_DIR` 环境变量，指向本地安装的 Windows x64 OpenSSL：

```
OPENSSL_LIB_DIR = E:\Programs\OpenSSL-Win64\lib\VC\x64\MD
OPENSSL_INCLUDE_DIR = E:\Programs\OpenSSL-Win64\include
```

当 Cargokit 为 Android（aarch64-linux-android、armv7-linux-androideabi）交叉编译时，`openssl-sys` 仍然读取这些环境变量，发现 Windows x64 的 .lib 文件与 Android 目标架构不匹配，直接报错：

```
OpenSSL libdir at `["E:\\Programs\\OpenSSL-Win64\\lib\\VC\\x64\\MD"]` does not contain
the required files to either statically or dynamically link OpenSSL
```

#### 原因 B：vendored 模式不可用

尝试启用 `openssl-sys` 的 `vendored` feature（从源码编译 OpenSSL），但在 Windows 上同样失败：

- **MSYS2 Perl（Git for Windows 自带）**：缺少 `Locale::Maketext::Simple`、`ExtUtils::MakeMaker` 等 Perl 模块，且无法通过 CPAN 安装（CPAN 模块本身也不完整）。
- **Strawberry Perl**：模块完整，但生成的路径使用 Windows 反斜杠，与 OpenSSL Configure 脚本期望的 Unix 正斜杠路径不兼容，报错：

```
This perl implementation doesn't produce Unix like paths (with forward slash
directory separators). Please use an implementation that matches your
building platform.
```

### 3. Cargokit 单目标失败导致全部中止

Cargokit 的 `ArtifactProvider.getArtifacts()` 方法在遍历所有 Android 目标时，如果第一个目标（armv7）构建失败抛出异常，整个方法终止，后续目标（arm64、x86_64）也不会构建。结果生成的 AAR（Android Archive）完全不包含任何 .so 文件，导致所有架构的 APK 都缺少 Rust 库。

### 4. main() 缺少错误处理

即使 Rust 库加载失败，`main()` 函数中的 `await RustLib.init()` 没有 try-catch 包裹。如果 Rust 库缺失或加载失败，应用会在原生白屏启动页面上静默崩溃，用户看不到任何错误信息。

---

## 解决方案

### 方案一：使用预编译的 Android OpenSSL 库（已采用）

核心思路：不从源码编译 OpenSSL，而是下载预编译的 Android OpenSSL 静态库，通过环境变量告诉 `openssl-sys` 使用它们。

#### 步骤 1：下载预编译库

从 [TaurusTLS-Developers/OpenSSL-Distribution](https://github.com/TaurusTLS-Developers/OpenSSL-Distribution/releases) 下载 Android 版本：

- `openssl-3.0.20-Android-arm.zip`（armeabi-v7a）
- `openssl-3.0.20-Android-arm64.zip`（arm64-v8a）

解压到 `ignite_pay_app/android-openssl/` 目录，结构如下：

```
ignite_pay_app/
├── android-openssl/
│   ├── arm/              # armeabi-v7a
│   │   ├── include/      # OpenSSL 头文件
│   │   │   └── openssl/
│   │   └── lib/          # 静态库
│   │       ├── libcrypto.a
│   │       └── libssl.a
│   └── arm64/            # arm64-v8a
│       ├── include/
│       └── lib/
```

注意：预编译包中 .a 文件可能在 `lib/static/` 子目录下，需要复制到 `lib/` 目录：

```bash
cp android-openssl/arm/lib/static/*.a android-openssl/arm/lib/
cp android-openssl/arm64/lib/static/*.a android-openssl/arm64/lib/
```

#### 步骤 2：修改 Cargokit 的 Android 环境变量

修改 `rust_builder/cargokit/build_tool/lib/src/android_environment.dart`，在 `buildEnvironment()` 方法中根据目标架构设置 `OPENSSL_DIR`、`OPENSSL_LIB_DIR`、`OPENSSL_INCLUDE_DIR`：

```dart
// Prebuilt OpenSSL for Android cross-compilation on Windows
String? opensslDir;
if (Platform.isWindows) {
  final manifestDir = Platform.environment['CARGOKIT_MANIFEST_DIR'];
  if (manifestDir != null) {
    final opensslBase = path.join(path.dirname(manifestDir), 'android-openssl');
    final archDir = target.android == 'arm64-v8a'
        ? 'arm64'
        : (target.android == 'armeabi-v7a' ? 'arm' : null);
    if (archDir != null) {
      final candidate = path.join(opensslBase, archDir);
      if (Directory(candidate).existsSync()) {
        opensslDir = candidate;
      }
    }
  }
}

return {
  // ... other env vars ...
  if (opensslDir != null) ...{
    'OPENSSL_DIR': opensslDir,
    'OPENSSL_LIB_DIR': path.join(opensslDir, 'lib'),
    'OPENSSL_INCLUDE_DIR': path.join(opensslDir, 'include'),
  } else ...{
    'OPENSSL_LIB_DIR': '',
    'OPENSSL_INCLUDE_DIR': '',
    'OPENSSL_DIR': '',
  },
};
```

**关键点**：必须同时覆盖 `OPENSSL_LIB_DIR` 和 `OPENSSL_INCLUDE_DIR`，否则 Windows 系统 OpenSSL 环境变量会通过 `includeParentEnvironment: true` 泄漏到子进程中。

#### 步骤 3：修改 Cargokit 容错机制

修改 `artifacts_provider.dart`，使单个目标构建失败不影响其他目标：

```dart
for (final target in targets) {
  // ...
  String targetDir;
  try {
    targetDir = await builder.build();
  } catch (e) {
    _log.warning('Build failed for $target, skipping: $e');
    continue;
  }
  // ...
}
```

修改 `build_gradle.dart`，处理缺失的目标产物：

```dart
for (final target in targets) {
  final libs = artifacts[target];
  if (libs == null) {
    log.warning('No artifacts for $target, skipping');
    continue;
  }
  // ... copy artifacts ...
}
```

#### 步骤 4：添加 main() 错误处理

修改 `lib/main.dart`，捕获 RustLib 初始化失败：

```dart
Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  try {
    await RustLib.init().timeout(const Duration(seconds: 10));
  } catch (e) {
    debugPrint('RustLib.init() failed: $e');
  }
  runApp(const IgnitePayApp());
}
```

在 `_AppShellState` 中添加初始化错误状态和重试 UI。

#### 步骤 5：更新 .gitignore

```gitignore
# Flutter / Dart
ignite_pay_app/android-openssl/
```

---

## 构建结果

| APK 文件 | 大小 | Rust .so | 说明 |
|----------|------|----------|------|
| `app-arm64-v8a-release.apk` | 32.0 MB | 7.1 MB | 主流 Android 设备使用 |
| `app-armeabi-v7a-release.apk` | 25.8 MB | 4.7 MB | 旧设备兼容 |
| `app-x86_64-release.apk` | 27.6 MB | 无 | 模拟器使用（缺少 x86_64 预编译 OpenSSL） |

---

## 其他尝试过的方案（未成功）

### 方案 B：openssl-sys vendored feature

在 `Cargo.toml` 中添加：

```toml
openssl-sys = { version = "0.9", features = ["vendored"] }
```

**失败原因**：
- MSYS2 Perl 缺少 `Locale::Maketext::Simple`、`ExtUtils::MakeMaker` 等模块
- Strawberry Perl 生成 Windows 路径格式，与 OpenSSL Configure 脚本不兼容
- 逐个安装 Perl 模块 stub 会引发连锁依赖问题

### 方案 C：清除系统 OpenSSL 环境变量

在 `android_environment.dart` 中将 `OPENSSL_LIB_DIR` 设为空字符串。

**失败原因**：`openssl-sys` 检测到空字符串后报错 `OpenSSL library directory does not exist: [""]`，因为没有其他 OpenSSL 源可用。

### 方案 D：仅跳过 armv7 目标

在 Gradle plugin 中过滤掉 `android-arm` 平台。

**失败原因**：所有 Android 目标（包括 arm64）都受系统 OpenSSL 环境变量影响，不仅仅是 armv7。

---

## 相关文件

| 文件 | 修改内容 |
|------|----------|
| `ignite_pay_app/rust_builder/cargokit/build_tool/lib/src/android_environment.dart` | 添加预编译 OpenSSL 路径逻辑 |
| `ignite_pay_app/rust_builder/cargokit/build_tool/lib/src/artifacts_provider.dart` | 单目标构建失败容错 |
| `ignite_pay_app/rust_builder/cargokit/build_tool/lib/src/build_gradle.dart` | 处理缺失的目标产物 |
| `ignite_pay_app/lib/main.dart` | 添加 RustLib.init() 错误处理和超时 |
| `ignite_pay_app/android-openssl/` | 预编译 Android OpenSSL 库（不提交到 Git） |
| `.gitignore` | 添加 `ignite_pay_app/android-openssl/` |

---

## 参考链接

- [TaurusTLS OpenSSL Distribution](https://github.com/TaurusTLS-Developers/OpenSSL-Distribution/releases) - 预编译 OpenSSL 二进制文件
- [openssl-sys crate](https://crates.io/crates/openssl-sys) - OpenSSL Rust 绑定
- [flutter_rust_bridge](https://cjycode.com/flutter_rust_bridge/) - Flutter Rust 互操作框架
- [Cargokit](https://github.com/irondash/cargokit) - Flutter Rust 插件构建系统
