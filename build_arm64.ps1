$env:JAVA_HOME = "C:\Users\qixua\AppData\Local\Android\Sdk\jdk-17"
$env:Path = "$env:JAVA_HOME\bin;" + $env:Path

if (-not $env:ANDROID_NDK_HOME) {
    $env:ANDROID_NDK_HOME = "C:\Users\qixua\AppData\Local\Android\Sdk\ndk\26.1.10909125"
}

$NDK_BIN = "$env:ANDROID_NDK_HOME\toolchains\llvm\prebuilt\windows-x86_64\bin"

$env:CC_aarch64_linux_android = "$NDK_BIN\aarch64-linux-android34-clang.cmd"
$env:AR_aarch64_linux_android = "$NDK_BIN\llvm-ar.exe"
$env:CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER = "$NDK_BIN\aarch64-linux-android34-clang.cmd"

Write-Host "Building ocn-core shared library (.so) for aarch64-linux-android..."
cargo build --target aarch64-linux-android --features android-jni --release -p ocn-core
